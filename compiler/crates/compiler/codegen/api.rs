//! The typed API games call: Fire, On, Invoke and the rest for every event and function.

use super::emitter::Emitter;
use super::module::Mode;
use super::reader::handlers_table;
use super::types::{Typed, arguments, call_type, fields_typed, handler_type, luau_type, parameters, returns};
use super::writer::{writer, writer_name};
use crate::layout::EventLayout;
use crate::luau::builtins::{ERROR, LEN};
use crate::luau::{Block, Entry, ExprId, Name, Parameter, Stmt, TypeBody, assign, local, when};
use crate::schema::{CALL_ID, Field, FunctionId, Role, Side, UNION_TAG};

const PICK_CALL: &str = concat!(
    "type function PickCall(Union: type, Name: type)\n",
    "\tlocal Variants = if Union:is(\"union\") then Union:components() else { Union }\n",
    "\tfor _, Variant in Variants do\n",
    "\t\tif Variant:readproperty(types.singleton(\"Name\")) == Name then\n",
    "\t\t\treturn Variant\n",
    "\t\tend\n",
    "\tend\n",
    "\tif Name:is(\"singleton\") then\n",
    "\t\terror(`no event is named {Name:value()}`)\n",
    "\tend\n",
    "\terror(\"Call takes an event's name in quotes\")\n",
    "end\n",
);

struct Method<'a> {
    key: &'static str,
    parameters: Vec<Typed<'a>>,
    returns: String,
    body: Block<'a>,
}

impl<'a> Method<'a> {
    fn new(key: &'static str, parameters: Vec<Typed<'a>>, returns: &str, body: Block<'a>) -> Method<'a> {
        Method { key, parameters, returns: returns.to_string(), body }
    }

    fn field_type(&self) -> String {
        let returns = if self.returns.is_empty() { "()" } else { &self.returns };
        let parameters: Vec<String> =
            self.parameters.iter().map(|typed| format!("{}: {}", typed.name, typed.ty)).collect();
        format!("{}: ({}) -> {returns}", self.key, parameters.join(", "))
    }
}

struct Api<'f, 'e, 'a> {
    e: &'f Emitter<'e, 'a>,
}

impl<'a> Api<'_, '_, 'a> {
    fn event_name(&self, event: &EventLayout) -> &'a str {
        &self.e.schema.events[event.event].name
    }

    fn closure(&self, parameters: Vec<Parameter<'a>>, returns: &str, body: Block<'a>) -> ExprId {
        self.e.b.function(parameters, returns.to_string(), body)
    }

    fn object(&self, prelude: Block<'a>, methods: Vec<Method<'a>>) -> (ExprId, String) {
        let b = self.e.b;
        let types: Vec<String> = methods.iter().map(Method::field_type).collect();
        let entries: Vec<Entry> = methods
            .into_iter()
            .map(|method| Entry {
                key: Some(method.key.into()),
                value: self.closure(parameters(&method.parameters), &method.returns, method.body),
            })
            .collect();
        let mut body = prelude;
        body.push(Stmt::Return(vec![b.table(entries, true)]));
        (b.call(self.closure(Vec::new(), "", body), []), format!("{{ {} }}", types.join(", ")))
    }

    fn names(&self, fields: &'a [Field<'a>]) -> Vec<ExprId> {
        fields.iter().map(|field| self.e.b.fixed(&field.name)).collect()
    }

    fn write_call(&self, event: &EventLayout, stream: ExprId, arguments: Vec<ExprId>) -> Stmt<'a> {
        let b = self.e.b;
        let mut all = vec![stream];
        all.extend(arguments);
        Stmt::Call(b.call(b.name(writer_name(self.e, event)), all))
    }

    fn wrong_side(&self, what: &str, server: bool) -> Stmt<'a> {
        let b = self.e.b;
        let message = format!("{what} is {} only", if server { "server" } else { "client" });
        Stmt::Call(b.invoke(ERROR, [b.string(message), b.number(2)]))
    }

    fn expect_side(&self, what: &str, server: bool) -> Stmt<'a> {
        let b = self.e.b;
        let wrong = if server { b.not(b.fixed("IS_SERVER")) } else { b.fixed("IS_SERVER") };
        when(wrong, vec![self.wrong_side(what, server)])
    }

    fn stream_index(&self, reliable: bool) -> ExprId {
        self.e.b.number(if reliable { 1 } else { 2 })
    }

    fn client_stream(&self, what: &str, reliable: bool) -> Block<'a> {
        let b = self.e.b;
        vec![
            local(Name::Fixed("Stream"), b.index(b.fixed("CLIENT_STREAMS"), self.stream_index(reliable))),
            when(b.eq(b.fixed("Stream"), b.nil()), vec![self.wrong_side(what, false)]),
        ]
    }

    fn player_stream(&self, streams: ExprId, reliable: bool) -> ExprId {
        self.e.b.index(streams, self.stream_index(reliable))
    }

    fn on(&self, event: &EventLayout, label: &str, receiver: Side, handler: String) -> Method<'a> {
        let b = self.e.b;
        let slot = || b.field(b.fixed(handlers_table(receiver.other())), self.event_name(event));
        let disconnect =
            self.closure(Vec::new(), "", vec![when(b.eq(slot(), b.fixed("Handler")), vec![assign(slot(), b.nil())])]);
        let body = vec![
            self.expect_side(&format!("{label}.On"), receiver == Side::Server),
            assign(slot(), b.fixed("Handler")),
            Stmt::Return(vec![disconnect]),
        ];
        Method::new("On", vec![Typed::new("Handler", handler)], "() -> ()", body)
    }

    fn add_middleware(&self, event: &EventLayout, label: &str, receiver: Side) -> Method<'a> {
        let b = self.e.b;
        let record = b.field(b.fixed("EVENTS"), self.event_name(event));
        let body = vec![
            self.expect_side(&format!("{label}.AddMiddleware"), receiver == Side::Server),
            Stmt::Return(vec![b.method(record, "AddMiddleware", [b.fixed("Middleware")])]),
        ];
        let call = call_type(self.e.schema, &self.e.schema.events[event.event]);
        Method::new("AddMiddleware", vec![Typed::new("Middleware", format!("(Call: {call}) -> ()"))], "() -> ()", body)
    }

    fn add_everywhere(&self) -> ExprId {
        let b = self.e.b;
        let adds = |from: Side| {
            let entries: Vec<Entry> = received(self.e)
                .filter(|event| self.e.schema.events[event.event].from == from)
                .map(|event| {
                    let record = b.field(b.fixed("EVENTS"), self.event_name(event));
                    Entry { key: None, value: b.method(record, "AddMiddleware", [b.fixed("Middleware")]) }
                })
                .collect();
            b.table(entries, true)
        };
        let removers = b.if_else(b.fixed("IS_SERVER"), adds(Side::Client), adds(Side::Server));
        let index = b.fixed("Index");
        let remove = Stmt::For(
            Name::Fixed("Index"),
            b.number(1),
            b.length(b.fixed("Removers")),
            vec![Stmt::Call(b.call(b.index(b.fixed("Removers"), index), []))],
        );
        let body = vec![
            Stmt::Local(Name::Fixed("Removers"), Some("{ () -> () }".into()), Some(removers)),
            Stmt::Return(vec![self.closure(Vec::new(), "", vec![remove])]),
        ];
        let parameters = vec![Parameter::new(Name::Fixed("Middleware"), "(Call: Calls) -> ()")];
        self.closure(parameters, "() -> ()", body)
    }

    fn fires(&self, event: &EventLayout) -> Vec<Method<'a>> {
        let (e, b) = (self.e, self.e.b);
        let declared = &e.schema.events[event.event];
        let label = self.event_name(event);
        let typed = fields_typed(e.schema, &declared.fields, true);
        let arguments = || self.names(&declared.fields);
        let with = |first: Typed<'a>| {
            let mut all = vec![first];
            all.extend(typed.iter().cloned());
            all
        };
        if declared.from == Side::Client {
            let mut body = self.client_stream(&format!("{label}.Fire"), event.reliable);
            body.push(self.write_call(event, b.fixed("Stream"), arguments()));
            return vec![Method::new("Fire", typed, "", body)];
        }
        let write_to = || self.write_call(event, self.player_stream(b.fixed("Streams"), event.reliable), arguments());
        let streams_of = || local(Name::Fixed("Streams"), b.index(b.fixed("PLAYER_STREAMS"), b.fixed("Recipient")));
        let fire = vec![
            streams_of(),
            when(
                b.eq(b.fixed("Streams"), b.nil()),
                vec![self.expect_side(&format!("{label}.Fire"), true), Stmt::Return(Vec::new())],
            ),
            write_to(),
        ];
        let index = || b.fixed("Index");
        let streams_at = || local(Name::Fixed("Streams"), b.index(b.fixed("STREAM_LIST"), index()));
        let kind = self.stream_index(event.reliable);
        let all_loop = |deliver: &dyn Fn() -> Stmt<'a>| {
            Stmt::For(
                Name::Fixed("Index"),
                b.number(1),
                b.length(b.fixed("STREAM_LIST")),
                vec![streams_at(), deliver()],
            )
        };
        let except_loop = |deliver: &dyn Fn() -> Stmt<'a>| {
            Stmt::For(
                Name::Fixed("Index"),
                b.number(1),
                b.length(b.fixed("PLAYER_LIST")),
                vec![when(
                    b.ne(b.index(b.fixed("PLAYER_LIST"), index()), b.fixed("Except")),
                    vec![streams_at(), deliver()],
                )],
            )
        };
        let list_loop = |deliver: &dyn Fn() -> Stmt<'a>| {
            Stmt::For(
                Name::Fixed("Index"),
                b.number(1),
                b.length(b.fixed("Recipients")),
                vec![
                    local(
                        Name::Fixed("Streams"),
                        b.index(b.fixed("PLAYER_STREAMS"), b.index(b.fixed("Recipients"), index())),
                    ),
                    when(b.fixed("Streams"), vec![deliver()]),
                ],
            )
        };
        let broadcast = !e.layout.tails(event.node).is_empty();
        let append = || {
            let stream = self.player_stream(b.fixed("Streams"), event.reliable);
            Stmt::Call(b.invoke("AppendTo", [stream, b.fixed("Scratch"), b.fixed("ScratchEnd")]))
        };
        let sent = |name: &str, few: ExprId, each: &dyn Fn(&dyn Fn() -> Stmt<'a>) -> Stmt<'a>| {
            let mut body = vec![self.expect_side(&format!("{label}.{name}"), true)];
            if !broadcast {
                body.push(each(&write_to));
                return body;
            }
            body.push(when(few, vec![each(&write_to), Stmt::Return(Vec::new())]));
            let first = b.index(b.index(b.fixed("STREAM_LIST"), b.number(1)), kind);
            body.push(local(Name::Fixed("Scratch"), b.invoke("Broadcast", [first])));
            body.push(self.write_call(event, b.fixed("Scratch"), arguments()));
            body.push(local(Name::Fixed("ScratchEnd"), b.invoke(LEN, [b.fixed("Scratch")])));
            body.push(each(&append));
            body
        };
        let below = |list: &'static str, count: u32| b.lt(b.length(b.fixed(list)), b.number(count));
        let all = sent("FireAll", below("STREAM_LIST", 2), &all_loop);
        let except = sent("FireExcept", below("PLAYER_LIST", 3), &except_loop);
        let list = sent("FireList", b.or(below("Recipients", 2), below("STREAM_LIST", 1)), &list_loop);
        vec![
            Method::new("Fire", with(Typed::new("Recipient", "Player")), "", fire),
            Method::new("FireAll", typed.clone(), "", all),
            Method::new("FireExcept", with(Typed::new("Except", "Player")), "", except),
            Method::new("FireList", with(Typed::new("Recipients", "{ Player }")), "", list),
        ]
    }

    fn event_object(&self, event: &EventLayout) -> (ExprId, String) {
        let declared = &self.e.schema.events[event.event];
        let receiver = declared.from.other();
        let handler = handler_type(self.e.schema, declared, Mode::Api).expect("a message has a handler");
        let mut methods = self.fires(event);
        methods.push(self.on(event, &declared.name, receiver, handler));
        methods.push(self.add_middleware(event, &declared.name, receiver));
        self.object(vec![writer(self.e, event, false)], methods)
    }

    fn reply(&self, request: &EventLayout, reply: &EventLayout, function: FunctionId) -> Stmt<'a> {
        let (e, b) = (self.e, self.e.b);
        let results = &e.schema.function(function).results;
        let mut body = match e.schema.events[request.event].from {
            Side::Client => vec![
                local(Name::Fixed("Streams"), b.index(b.fixed("PLAYER_STREAMS"), b.cast(b.fixed("Peer"), "Player"))),
                when(b.eq(b.fixed("Streams"), b.nil()), vec![Stmt::Return(Vec::new())]),
                local(Name::Fixed("Stream"), self.player_stream(b.fixed("Streams"), true)),
            ],
            Side::Server => {
                vec![local(Name::Fixed("Stream"), b.index(b.fixed("CLIENT_STREAMS"), self.stream_index(true)))]
            }
        };
        let union = |variant: &'a str, fields: Vec<Entry<'a>>| {
            let mut entries = vec![Entry { key: Some(UNION_TAG.into()), value: b.string(variant) }];
            entries.extend(fields);
            b.table(entries, false)
        };
        let mut succeeded = Vec::new();
        if !results.is_empty() {
            let names = results.iter().map(|field| Name::Fixed(&field.name)).collect();
            succeeded.push(Stmt::Locals(names, None, b.vararg()));
        }
        let fields =
            results.iter().map(|field| Entry { key: Some(field.name.as_ref().into()), value: b.fixed(&field.name) });
        succeeded.push(self.write_call(
            reply,
            b.fixed("Stream"),
            vec![b.fixed(CALL_ID), union("Ok", fields.collect())],
        ));
        let reason = vec![Entry { key: Some("Reason".into()), value: b.vararg() }];
        let failed = vec![self.write_call(reply, b.fixed("Stream"), vec![b.fixed(CALL_ID), union("Error", reason)])];
        body.push(Stmt::If(vec![(b.fixed("Success"), succeeded)], Some(failed)));
        let parameters = vec![
            Parameter::new(Name::Fixed("Peer"), "Player?"),
            Parameter::new(Name::Fixed(CALL_ID), "number"),
            Parameter::new(Name::Fixed("Success"), "boolean"),
            Parameter::vararg(),
        ];
        let target = b.field(b.field(b.fixed("EVENTS"), self.event_name(request)), "Reply");
        assign(target, self.closure(parameters, "", body))
    }

    fn function_object(&self, request: &EventLayout, reply: &EventLayout, function: FunctionId) -> (ExprId, String) {
        let (e, b) = (self.e, self.e.b);
        let declared = &e.schema.events[request.event];
        let label: &str = &e.schema.function(function).name;
        let arguments = &declared.fields[1..];
        let typed = fields_typed(e.schema, arguments, true);
        let future = format!("Future<{}>", returns(e.schema, &e.schema.function(function).results, false));
        let send = |stream: ExprId| {
            let mut all = vec![b.fixed(CALL_ID)];
            all.extend(self.names(arguments));
            let parameters = vec![Parameter::new(Name::Fixed(CALL_ID), "number")];
            self.closure(parameters, "", vec![self.write_call(request, stream, all)])
        };
        let (invoke_parameters, invoke_body) = if declared.from == Side::Client {
            let mut body = self.client_stream(&format!("{label}.Invoke"), true);
            let invoked = b.invoke("INVOKE", [b.nil(), send(b.fixed("Stream"))]);
            body.push(Stmt::Return(vec![b.cast(invoked, "any")]));
            (typed, body)
        } else {
            let mut all = vec![Typed::new("Recipient", "Player")];
            all.extend(typed);
            let stream = self.player_stream(b.index(b.fixed("PLAYER_STREAMS"), b.fixed("Recipient")), true);
            let invoked = b.invoke("INVOKE", [b.fixed("Recipient"), send(stream)]);
            let body =
                vec![self.expect_side(&format!("{label}.Invoke"), true), Stmt::Return(vec![b.cast(invoked, "any")])];
            (all, body)
        };
        let receiver = declared.from.other();
        let handler = handler_type(e.schema, declared, Mode::Api).expect("a request has a handler");
        let methods = vec![
            Method::new("Invoke", invoke_parameters, &future, invoke_body),
            self.on(request, label, receiver, handler),
            self.add_middleware(request, label, receiver),
        ];
        let prelude = vec![writer(e, request, false), writer(e, reply, false), self.reply(request, reply, function)];
        self.object(prelude, methods)
    }
}

fn received<'e, 'a>(e: &'e Emitter<'_, 'a>) -> impl Iterator<Item = &'e EventLayout> {
    e.layout.events().map(|(_, event)| event).filter(|event| !matches!(event.role, Role::Reply(_)))
}

pub fn middleware_types<'a>(e: &Emitter<'_, 'a>, front: bool) -> Vec<Stmt<'a>> {
    let calls: Vec<String> = received(e).map(|event| call_type(e.schema, &e.schema.events[event.event])).collect();
    if calls.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    if front {
        out.extend([Stmt::Verbatim(PICK_CALL.into()), Stmt::Blank]);
    }
    let union = TypeBody::Inline(calls.join("\n\t| ").into());
    out.push(Stmt::TypeAlias { name: "Calls".into(), generics: "", body: union });
    if front {
        let pick = TypeBody::Inline("PickCall<Calls, N>".into());
        out.push(Stmt::TypeAlias { name: "Call".into(), generics: "<N>", body: pick });
    }
    out.push(Stmt::Blank);
    out
}

fn pack_and_unpack<'a>(e: &Emitter<'_, 'a>, event: &EventLayout) -> [ExprId; 2] {
    let b = e.b;
    let fields = arguments(&e.schema.events[event.event]);
    let pack_parameters =
        fields.iter().map(|field| Parameter::new(Name::Fixed(&field.name), luau_type(e.schema, field.ty, false)));
    let packed =
        fields.iter().map(|field| Entry { key: Some(field.name.as_ref().into()), value: b.fixed(&field.name) });
    let pack = b.function(
        pack_parameters.collect(),
        String::new(),
        vec![Stmt::Return(vec![b.table(packed.collect::<Vec<_>>(), false)])],
    );
    let unpacked = fields.iter().map(|field| b.field(b.fixed("Args"), &field.name)).collect();
    let unpack_parameters = vec![Parameter::new(Name::Fixed("Args"), "{ [string]: unknown }")];
    let unpack = b.function(unpack_parameters, String::new(), vec![Stmt::Return(unpacked)]);
    [pack, unpack]
}

pub fn bindings<'a>(e: &Emitter<'_, 'a>) -> Block<'a> {
    let b = e.b;
    let from_runtime =
        |name: &'static str, field: &'static str| local(Name::Fixed(name), b.field(b.fixed("Courier"), field));
    let sends = |from: Side, role: fn(&Role) -> bool| {
        e.layout.events().any(|(direction, event)| direction.from == from && role(&event.role))
    };
    let mut out = vec![from_runtime("IS_SERVER", "IsServer")];
    if sends(Side::Server, |_| true) {
        out.push(from_runtime("PLAYER_STREAMS", "PlayerStreams"));
    }
    if sends(Side::Server, |role| *role == Role::Message) {
        out.push(from_runtime("PLAYER_LIST", "PlayerList"));
        out.push(from_runtime("STREAM_LIST", "StreamList"));
    }
    if sends(Side::Client, |_| true) {
        out.push(from_runtime("CLIENT_STREAMS", "ClientStreams"));
    }
    if !e.schema.functions.is_empty() {
        out.push(from_runtime("INVOKE", "Invoke"));
        out.push(from_runtime("COMPLETE", "Complete"));
    }
    let events = received(e).map(|event| {
        let name: &'a str = &e.schema.events[event.event].name;
        let [pack, unpack] = pack_and_unpack(e, event);
        let record = b.invoke("Courier.Event", [b.string(name), pack, unpack]);
        Entry { key: Some(name.into()), value: record }
    });
    out.push(local(Name::Fixed("EVENTS"), b.table(events.collect::<Vec<_>>(), true)));
    out.push(Stmt::Blank);
    out
}

pub fn api<'a>(e: &Emitter<'_, 'a>) -> (Stmt<'a>, Vec<Stmt<'a>>) {
    let api = Api { e };
    let mut entries = Vec::new();
    let mut types = Vec::new();
    for (_, event) in e.layout.events() {
        let (key, (value, ty)): (&'a str, _) = match event.role {
            Role::Message => (&e.schema.events[event.event].name, api.event_object(event)),
            Role::Request(function) => {
                let (_, reply) = e
                    .layout
                    .events()
                    .find(|(_, other)| other.role == Role::Reply(function))
                    .expect("every request has a reply");
                (&e.schema.function(function).name, api.function_object(event, reply, function))
            }
            Role::Reply(_) => continue,
        };
        entries.push(assign(e.b.field(e.b.fixed("API"), key), value));
        types.push((key.into(), ty.into()));
    }
    if received(e).next().is_some() {
        entries.push(assign(e.b.field(e.b.fixed("API"), "AddMiddleware"), api.add_everywhere()));
        types.push(("AddMiddleware".into(), "(Middleware: (Call: Calls) -> ()) -> () -> ()".into()));
    }
    let alias = Stmt::TypeAlias { name: "Api".into(), generics: "", body: TypeBody::Fields(types) };
    let mut table =
        vec![Stmt::Local(Name::Fixed("API"), Some("Api".into()), Some(e.b.cast(e.b.table(Vec::new(), false), "any")))];
    table.extend(entries);
    (alias, table)
}
