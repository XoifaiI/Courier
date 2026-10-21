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

| | Bytes per frame | Server time per frame |
|---|---|---|
| **Courier** | **863** | **10 µs** |
| BlinkBlox | 865 | 17 µs |
| Blink | 865 | 29 µs |
| QuickNetRaised | 868 | 24 µs |
| BitNet | 873 | 25 µs |

Courier writes each message when you fire it, so its 10 µs is all of its work. Some of the others
finish theirs later in the frame.

**Attack**, Random exploit gibberish sent

| | Server errors | Warnings | Bad values handed to the game | Longest frame |
|---|---|---|---|---|
| **Courier** | **0** | **0** | **0** | 27 ms |
| BlinkBlox | 0 | 3 | 6 | 26 ms |
| QuickNetRaised | 0 | 179 | 9 | 32 ms |
| Blink | 156 | 0 | 6 | 47 ms |
| BitNet | 0 | 233 | 0 | 213 ms |

Bad values include NaN vectors and broken tables that reached a handler. Bandwidth in the throughput
test isn't shown: every message is the same, so Roblox compresses them all to about one. The
[bench](bench) explains the rest.

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
