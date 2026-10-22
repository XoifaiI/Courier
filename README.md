<div align="center">

# Courier
[![GitHub](https://img.shields.io/badge/GitHub-Courier-181717?style=for-the-badge&logo=github&logoColor=white)](https://github.com/XoifaiI/Courier)

</div>

## What is this?

A networking library for Roblox that sends the fewest bytes it can, as fast as it can. You design
a schema, and the Courier compiler turns it into strictly typed Luau that packs every value into
exactly the bits it needs and validates every value it reads.

```luau
option output = "src/shared/Net"

enum Weapon { Sword, Bow, Staff }

event Shoot from Client {
	Weapon: Weapon,
	Target: Instance(BasePart),
	Aim: vector(-1..1, 0.001),
}

event Damaged from Server unreliable {
	Amount: int(0..100),
	Health: int(0..100),
}

function Buy from Client { Item: u16, Count: u8 } -> { Granted: bool, Balance: u32 }
```

```luau
local Net = require(ReplicatedStorage.Net)

-- client
Net.Shoot.Fire("Bow", Target, Camera.CFrame.LookVector)
local Granted, Balance = Net.Buy.Invoke(5, 3):Wait()

-- server
Net.Shoot.On(function(Player, Weapon, Target, Aim)
	print(Player, Weapon, Target, Aim)
end)

Net.Buy.On(function(Player, Item, Count)
	return true, 985
end)

Net.Damaged.FireAll(10, 90)
```

`Shoot` is 37 bits, id included. `Damaged` is 16. The part costs nothing, it goes alongside.

Clients can send anything, so every value is checked as it's read. If one is wrong, the whole
frame is dropped and your handler never sees it.

## Features

- **Bit packed** | a bool is 1 bit, `int(0..100)` is 7
- **One call per frame** | everything a player gets in a frame goes out together
- **Typed** | every Fire and On is typed from the schema
- **Validated** | bad values never reach a handler
- **Fast** | generated for `--!native`
- **Functions** | `Invoke` returns a Future
- **Instances** | wait on parts that haven't streamed in yet
- **Middleware** | per event, rate limiter included
- **Editor support** | hover an event to see its size, errors as you type
- **Watch mode** | recompiles when you save

## Benchmarks

Run in Studio with the [bench](bench), which you can run yourself. All 16 libraries it covers ran;

**Throughput**, client frames per second while firing each test 1000 times a frame. Higher is better.

| | Vectors | Numbers | Dict | Booleans | Entities | Strings |
|---|---|---|---|---|---|---|
| **Courier** | **203** | **106** | **56** | **124** | **64** | **57** |
| QuickNetRaised | 180 | 103 | 54 | 121 | 48 | 16 (lost 41%) |
| BitNet | 160 | 104 | 41 | 39 | 41 | 44 |
| NetRayCompile | 164 | 51 | 42 | 93 | 31 | 54 |
| BlinkBlox | 128 | 76 | 49 | 93 | 62 | 47 |
| Blink | 155 | 76 | 52 | 66 | 31 | 52 |
| Satset | 145 | 95 | 15 | 102 | 20 | 47 |
| Zap | 103 | 59 | 52 | 58 | 28 | 48 |
| Lync | 51 | 31 | 15 | 58 | 28 | 45 |
| Roblox remotes | 70 | 15 | 18 | 15 | too slow | 32 |

**Game**, a server sending 50 moving things and a few events to a player every frame.

| | Bytes per frame | Remote calls per frame | Server time per frame |
|---|---|---|---|
| **Courier** | 863 | 1.00 | **53 µs** |
| BlinkBlox | 865 | 1.00 | 54 µs |
| QuickNet | 868 | 0.99 | 56 µs |
| Satset | 869 | 0.99 | 68 µs |
| Blink | 865 | 1.00 | 72 µs |
| ByteNet | 870 | 1.00 | 74 µs |
| NetRayCompile | 865 | 3.06 | 77 µs |
| Zap | 865 | 1.00 | 81 µs |
| BitNet | 874 | 1.01 | 82 µs |
| Packet | 866 | 1.00 | 98 µs |
| Lync | 867 | 1.00 | 109 µs |
| Squash | 862 | 3.06 | 114 µs |
| PakNet | 874 | 3.06 | 136 µs |
| Roblox remotes | 3240 | 3.06 | 162 µs |

No library lost a message or handed the game a bad value. Courier writes each message when you fire
it, so its 53 µs is all of its work. Some of the others finish theirs later in the frame. BitNet
sent about a quarter as many messages as the others in the same time.

**Attack**, a client sending everything an exploiter could: junk values, random, cut and flipped
buffers, deep and sparse tables, missing and extra arguments, 8000 arguments, and broken JSON.

| | Server errors | Warnings | Bad values handed to the game | Longest frame | Memory peak |
|---|---|---|---|---|---|
| **Courier** | **0** | **0** | **0** | 317 ms | +60 KB |
| Satset | 0 | 0 | 0 | 311 ms | +3 KB |
| BitNet | 0 | 894 | 0 | 310 ms | 0 KB |
| Packet | 0 | 317 | 1 | 295 ms | +82 KB |
| BlinkBlox | 0 | 6 | 2 | 317 ms | +143 KB |
| Lync | 0 | 90 | 2 | 315 ms | +4 KB |
| QuickNet | 0 | 504 | 6 | 548 ms | 0 KB |
| ByteNet | 90 | 0 | 1 | 323 ms | 0 KB |
| NetRayCompile | 165 | 0 | 4 | 310 ms | 0 KB |
| Squash | 187 | 0 | 8 | 565 ms | +851 MB |
| Zap | 420 | 0 | 3 | 326 ms | +3 KB |
| Blink | 469 | 1 | 2 | 311 ms | +201 KB |
| PakNet | 916 | 0 | 2 | 316 ms | +364 KB |
| Roblox remotes | 0 | 0 | 919 | 301 ms | 0 KB |

Bad values include NaN vectors and broken tables that reached a handler. Memory
peak is the most the library's own scripts held over what they held before. Squash reads an array
length from the packet and allocates it before checking the bytes are there, so 826 KB of junk
buffers made it hold 851 MB.

<details>
<summary>Server errors / bad values by attack</summary>

| | Junk | Buffers | Truncated | Flipped | Overwritten | Tables | Sparse | Arguments | Nil arguments | 8000 arguments | JSON |
|---|---|---|---|---|---|---|---|---|---|---|---|
| **Courier** | - | - | - | - | - | - | - | - | - | - | - |
| Satset | - | - | - | - | - | - | - | - | - | - | - |
| BitNet | - | - | - | - | - | - | - | - | - | - | - |
| Packet | - | - | - | - | 0 / 1 | - | - | - | - | - | - |
| BlinkBlox | - | - | - | - | 0 / 2 | - | - | - | - | - | - |
| Lync | - | - | - | - | 0 / 2 | - | - | - | - | - | - |
| QuickNet | - | - | - | 0 / 1 | 0 / 5 | - | - | - | - | - | - |
| ByteNet | - | 34 / 0 | 28 / 0 | 1 / 0 | 16 / 1 | - | - | 11 / 0 | - | - | - |
| NetRayCompile | 20 / 0 | 22 / 0 | 42 / 0 | 10 / 0 | 3 / 4 | 9 / 0 | - | 7 / 0 | 7 / 0 | 9 / 0 | 36 / 0 |
| Squash | - | 91 / 0 | 29 / 4 | 7 / 0 | 28 / 4 | - | - | 32 / 0 | - | - | - |
| Zap | 20 / 0 | 19 / 0 | 42 / 0 | 11 / 0 | 30 / 3 | 9 / 0 | - | 12 / 0 | 25 / 0 | - | 252 / 0 |
| Blink | 39 / 0 | 9 / 0 | 42 / 0 | 16 / 0 | 17 / 2 | 15 / 0 | - | 11 / 0 | 32 / 0 | - | 288 / 0 |
| PakNet | 120 / 0 | 120 / 0 | 48 / 0 | 12 / 0 | 28 / 2 | 54 / 0 | - | 42 / 0 | 60 / 0 | - | 432 / 0 |
| Roblox remotes | 0 / 120 | 0 / 120 | - | - | - | 0 / 145 | 0 / 14 | 0 / 42 | 0 / 60 | - | 0 / 418 |

</details>

Bandwidth in the throughput test isn't shown: every message is the same, so Roblox compresses them
all to about one. The [bench](bench) explains the rest.

## Installing

**Compiler**, with [Rokit](https://github.com/rojo-rbx/rokit):

```sh
rokit add XoifaiI/Courier
```

Or grab a build from the [releases](https://github.com/XoifaiI/Courier/releases).

**Runtime**, with Wally:

```toml
[dependencies]
Courier = "xoifaii/courier@0.1.0"
```

Point your schema at it and start the compiler:

```luau
option runtime = "ReplicatedStorage.Packages.Courier"
```

```sh
courier watch
```

**No Rojo?** `courier Net.courier Net.rbxmx` gives you one model with everything in it. Insert it
into ReplicatedStorage.

**VS Code**: grab the `.vsix` from the [releases](https://github.com/XoifaiI/Courier/releases) and
run `code --install-extension courier-<version>.vsix`.

## License

This project has the [MIT License](https://github.com/XoifaiI/Courier/blob/main/LICENSE).
