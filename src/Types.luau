--!strict

--[=[
	Types: the shapes the runtime, middleware and generated schema modules share

	Return type: nothing; require it for its exported types
--]=]

export type function Decompose(generic: type)
	if generic:is("nil") then
		return types.any
	end

	local invalid: { typeof(generic.tag) } = { "nil", "unknown" }
	if table.find(invalid, generic.tag) then
		return types.never
	end

	if generic:is("function") then
		local head = generic:returns().head
		assert(head and head[1] ~= nil, `expected function to return one argument`)

		return head[1]
	end

	return generic
end

export type Packed = { n: number, [number]: any }

type FutureImpl<T...> = {
	__index: FutureImpl<T...>,

	Wait: (self: Future<T...>, Timeout: number?) -> T...,
	Cancel: (self: Future<T...>) -> (),
	Consume: (self: Future<T...>) -> (),

	Data: (self: Future<T...>) -> T...,
	Happened: (self: Future<T...>) -> boolean,
	Cancelled: (self: Future<T...>) -> (boolean, any),
}

export type Future<T...> = setmetatable<{}, FutureImpl<T...>>

export type Result<T, E> =
	{
		read Result: "Success",
		read Value: T,
	}
	| {
		read Result: "Error",
		read Reason: E,
	}

export type Origin = "Server" | "Client"
export type Direction = "Inbound" | "Outbound"
export type Metadata = {
	read Origin: Origin,
	read Player: Player?,
	read Direction: Direction,
}

export type Call = {
	read Shared: { [any]: any },
	read Metadata: Metadata,
	read Args: { any },
	read Count: number,
	State: Result<any, any>,

	read Reject: (self: Call, Reason: any) -> (),
}

export type MiddlewareCallback<T> = (Call, Decompose<T>) -> ()

type MiddlewareImpl<T> = {
	__index: MiddlewareImpl<T>,
	GetDataFor: (self: Middleware<T>, EventName: string) -> T,
}

export type Middleware<T> = setmetatable<{
	Template: T,
	EventData: { [string]: T },
	Callback: MiddlewareCallback<T>,
}, MiddlewareImpl<T>>

export type Middlewares = {
	read Ratelimiter: Middleware<RateLimiter>,
}

export type Bucket = {
	Tokens: number,
	Last: number,
}

export type RateLimiter = {
	Capacity: number,
	RefillPerSecond: number,
	Buckets: { [Player]: Bucket },
}

export type Reply = (Peer: Player?, CallId: number, Success: boolean, ...any) -> ()

type EventImpl = {
	__index: EventImpl,
	AddMiddleware: (self: Event, Middleware: Middleware<any>) -> (),
	RunMiddlewares: (self: Event, Sender: Player?, Args: { any }) -> (Call?, any),
	Handle: (self: Event, Handler: (...any) -> (), Sender: Player?, ...any) -> (),
	Respond: (self: Event, Handler: (...any) -> ...any, Sender: Player?, CallId: number, ...any) -> (),
	HandleInvoke: (self: Event, Handler: any, Sender: Player?, CallId: number, ...any) -> (),
}

export type Event = setmetatable<{
	read Name: string,
	read Count: number,
	read Middlewares: { Middleware<any> },
	read Data: { any },
	read Call: { __index: any },
	Reply: Reply?,
}, EventImpl>

export type Stream = buffer

export type Schema = {
	NewStream: (Capacity: number) -> Stream,
	SetOnFull: (Handler: (Stream: Stream) -> ()) -> (),
	IsEmpty: (Stream: Stream) -> boolean,
	Finish: (Stream: Stream) -> (buffer, { any }?),
	ReadOnServer: (Frame: buffer, Sender: Player, Received: { any }?) -> boolean,
	ReadOnClient: (Frame: buffer, Received: { any }?) -> boolean,
}

export type Config = {
	ReliableCapacity: number,
	UnreliableCapacity: number,
	InvokeTimeout: number,
	MaxFrameBytes: number,
	OnFrame: ((Player: Player, Success: boolean) -> ())?,
}

return nil
