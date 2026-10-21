//! Writes a .rbxmx model holding the runtime and the API, for people not using Rojo.

use courier_compiler::codegen::File;

const RUNTIME: Module = Module {
    name: "Courier",
    source: include_str!("../../src/init.luau"),
    children: &[
        Child::Module(Module { name: "Future", source: include_str!("../../src/Future.luau"), children: &[] }),
        Child::Module(Module { name: "Result", source: include_str!("../../src/Result.luau"), children: &[] }),
        Child::Module(Module { name: "Types", source: include_str!("../../src/Types.luau"), children: &[] }),
        Child::Module(Module {
            name: "Middlewares",
            source: include_str!("../../src/Middlewares/init.luau"),
            children: &[Child::Module(Module {
                name: "Ratelimiter",
                source: include_str!("../../src/Middlewares/Ratelimiter.luau"),
                children: &[],
            })],
        }),
        Child::Folder {
            name: "Remotes",
            children: &[
                Child::Instance { class: "RemoteEvent", name: "Reliable" },
                Child::Instance { class: "UnreliableRemoteEvent", name: "Unreliable" },
            ],
        },
    ],
};

struct Module {
    name: &'static str,
    source: &'static str,
    children: &'static [Child],
}

enum Child {
    Module(Module),
    Folder { name: &'static str, children: &'static [Child] },
    Instance { class: &'static str, name: &'static str },
}

struct Writer {
    out: String,
    referent: u32,
}

impl Writer {
    fn open(&mut self, class: &str, name: &str, depth: usize) {
        let indent = "\t".repeat(depth);
        self.out.push_str(&format!("{indent}<Item class=\"{class}\" referent=\"RBX{}\">\n", self.referent));
        self.referent += 1;
        self.out.push_str(&format!(
            "{indent}\t<Properties>\n{indent}\t\t<string name=\"Name\">{}</string>\n",
            escape(name)
        ));
    }

    fn source(&mut self, source: &str, depth: usize) {
        let indent = "\t".repeat(depth);
        let text = source.replace("]]>", "]]]]><![CDATA[>");
        self.out
            .push_str(&format!("{indent}\t\t<ProtectedString name=\"Source\"><![CDATA[{text}]]></ProtectedString>\n"));
    }

    fn close_properties(&mut self, depth: usize) {
        self.out.push_str(&format!("{}\t</Properties>\n", "\t".repeat(depth)));
    }

    fn close(&mut self, depth: usize) {
        self.out.push_str(&format!("{}</Item>\n", "\t".repeat(depth)));
    }

    fn module(&mut self, module: &Module, depth: usize) {
        self.open("ModuleScript", module.name, depth);
        self.source(module.source, depth);
        self.close_properties(depth);
        for child in module.children {
            self.child(child, depth + 1);
        }
        self.close(depth);
    }

    fn child(&mut self, child: &Child, depth: usize) {
        match child {
            Child::Module(module) => self.module(module, depth),
            Child::Folder { name, children } => {
                self.open("Folder", name, depth);
                self.close_properties(depth);
                for child in *children {
                    self.child(child, depth + 1);
                }
                self.close(depth);
            }
            Child::Instance { class, name } => {
                self.open(class, name, depth);
                self.close_properties(depth);
                self.close(depth);
            }
        }
    }
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

pub fn model(name: &str, files: &[File]) -> String {
    let mut writer = Writer { out: String::new(), referent: 0 };
    writer.out.push_str("<roblox version=\"4\">\n");
    writer.module(&RUNTIME, 1);
    let front = files.iter().find(|file| file.path == "init.luau").map_or("", |file| file.text.as_str());
    writer.open("ModuleScript", name, 1);
    writer.source(front, 1);
    writer.close_properties(1);
    for file in files.iter().filter(|file| file.path != "init.luau") {
        let child = file.path.trim_end_matches(".luau");
        writer.open("ModuleScript", child, 2);
        writer.source(&file.text, 2);
        writer.close_properties(2);
        writer.close(2);
    }
    writer.close(1);
    writer.out.push_str("</roblox>\n");
    writer.out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn holds_the_runtime_and_the_api() {
        let files = [
            File { path: "init.luau", text: "return require(\"@self/Implementation\")".into() },
            File { path: "Implementation.luau", text: "local T = a[b[c]]>d\nreturn {}".into() },
        ];
        let model = model("Net", &files);
        assert!(model.contains("<string name=\"Name\">Courier</string>"));
        assert!(model.contains("<Item class=\"UnreliableRemoteEvent\""));
        assert!(model.contains("<string name=\"Name\">Implementation</string>"));
        assert!(model.contains("a[b[c]]]]><![CDATA[>d"));
        assert_eq!(model.matches("<Item ").count(), model.matches("</Item>").count());
    }
}
