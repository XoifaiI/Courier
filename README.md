<div align="center">

# Courier
[![GitHub](https://img.shields.io/badge/GitHub-Courier-181717?style=for-the-badge&logo=github&logoColor=white)](https://github.com/XoifaiI/Courier) [![Docs](https://img.shields.io/badge/Docs-Read-8B5CF6?style=for-the-badge)](https://xoifaii.github.io/CourierDocs/) [![Benchmarks](https://img.shields.io/badge/Benchmarks-View-FF6A3D?style=for-the-badge)](https://xoifaii.github.io/CourierDocs/docs/benchmarks/)

</div>

## What is this?

A networking library for Roblox that sends the fewest bytes it can, as fast as it can. You design
a schema, and the Courier compiler turns it into strictly typed Luau that packs every value into
exactly the bits it needs and validates every value it reads.

```luau
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
frame is yeeted and your handler never sees it.

## Features

- **Bit packed** | a bool is 1 bit, `int(0..100)` is 7
- **One call per frame** | everything a player gets in a frame goes out together
- **Typed** | every Fire and On is typed from the schema
- **Validated** | bad values never reach a handler
- **Fast** | generated for `--!native`
- **Functions** | `Invoke` returns a Future
- **Instances** | wait on parts that haven't streamed in yet
- **Middleware** | typed per event, or on every event at once
- **Editor support** | hover an event to see its size, errors as you type; `courier vscode` installs it
- **Watch mode** | recompiles when you save

## License

This project has the [MIT License](https://github.com/XoifaiI/Courier/blob/main/LICENSE).
