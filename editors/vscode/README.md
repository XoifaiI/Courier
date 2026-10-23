# Courier for VS Code

Language support for [Courier](https://github.com/XoifaiI/Courier) schemas, the `.courier` files
the Courier compiler turns into bit packed, strictly typed Roblox networking.

## Features

- **Sizes as you type.** Every field shows the bits it costs, and every event shows the size of
  one message.
- **Live errors**, in the file they are in, across every schema of the folder.
- **One folder, one schema.** Types declared in one file work in the others, so hover, go to
  definition, find references and completion all follow them.
- **Rename** an enum, struct, union, event, function or field everywhere it is used. A rename
  that would break the schema is refused.
- **Completion** that fits where you are: declarations at the top level, `from`, `Server` or
  `Client`, `unreliable`, and every type after a `:`.
- **Compile on save.** Saving a schema compiles its folder into the module Rojo syncs, so you
  do not need `courier watch` running.

## Requirements

The extension runs the `courier` compiler. Install it with [Rokit](https://github.com/rojo-rbx/rokit):

```sh
rokit add XoifaiI/Courier
```

## Settings

| Setting | Default | What it does |
|---|---|---|
| `courier.path` | `courier` | The compiler to run, a name on PATH or a full path |
| `courier.compileOnSave` | `true` | Compile a schema's folder whenever a schema in it is saved |

Inline sizes are VS Code inlay hints. Turn them off with `editor.inlayHints.enabled`.
