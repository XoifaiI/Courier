<div align="center">

# Courier
[![GitHub](https://img.shields.io/badge/GitHub-Courier-181717?style=for-the-badge&logo=github&logoColor=white)](https://github.com/XoifaiI/Courier)

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
frame is dropped and your handler never sees it.

## Features

- **Bit packed** | a bool is 1 bit, `int(0..100)` is 7
- **One call per frame** | everything a player gets in a frame goes out together
- **Typed** | every Fire and On is typed from the schema
- **Validated** | bad values never reach a handler
- **Fast** | generated for `--!native`
- **Functions** | `Invoke` returns a Future
- **Instances** | wait on parts that haven't streamed in yet
- **Middleware** | typed per event, or on every event at once
- **Editor support** | hover an event to see its size, errors as you type
- **Watch mode** | recompiles when you save

## Benchmarks

Run in Studio with the [bench](bench), keep in mind these numbers are always gonna change from gc to native codegen, 
but its safe to safe it is competitive with others.

**Throughput**, client frames per second while firing each test 250 times a frame. Higher is better.

|                | Vectors | Numbers |        Dict | Booleans | Entities |     Strings |
|----------------|---------|---------|-------------|----------|----------|-------------|
| **Courier**    | **165** | **146** |      **55** |  **163** |  **116** |      **37** |
| QuickNet       | **165** |     142 |      **55** | 	  160 |       99 |          35 |
| BlinkBlox      |     160 |     116 |          52 |      138 |      100 |          34 |
| NetRayCompile  |     163 |     114 |          53 |      152 |       80 |          35 |
| Blink          |     164 |     128 |          53 |      124 |       80 |          36 |
| Satset         |     164 |     134 |          39 |      144 |       60 |          32 |
| BitNet         | **165** |     124 |          50 |       87 |       81 |          31 |
| Zap            |     153 |     118 |          52 |      106 |       71 |          35 |
| Lync           |     124 |      90 |          34 |      108 |       70 |          32 |
| ByteNet        |     160 |      97 | unsupported |       81 |       75 | unsupported |
| Packet         |     142 |      60 | unsupported |       70 |       53 |          33 |
| Squash         |      89 |      80 |          35 |      116 |       49 |          28 |
| PakNet         |      74 |      60 |          34 |       48 |       52 |          25 |
| Roblox remotes |     136 |      30 |          29 |       31 |       18 |          26 |

<details>
<summary>Full throughput results</summary>

**Vectors**

|                | FPS | FPS P10 to P90 | Bytes per message | KB/s at 60 FPS | Lost | Client bytes allocated per message | Server heap rise MB |
|----------------|-----|----------------|-------------------|----------------|------|------------------------------------|---------------------|
| **Courier**    | 166 |     166 to 178 |              1203 |          16091 | 0.0% |                                  0 |                16.1 |
| BitNet         | 165 |     164 to 176 |              1202 |          16076 | 0.0% |                                  0 |                14.5 |
| QuickNet       | 165 |     157 to 173 |              1203 |          15684 | 0.0% |                                 66 |                25.7 |
| Satset         | 164 |     160 to 168 |              1202 |          16302 | 0.0% |                                  0 |                49.1 |
| Blink          | 164 |     153 to 165 |              1203 |          16072 | 0.0% |                               3817 |                62.3 |
| NetRayCompile  | 163 |     155 to 168 |              1202 |          16381 | 0.0% |                                  0 |                54.2 |
| ByteNet        | 160 |     146 to 164 |              1203 |          16115 | 0.0% |                                  0 |                58.6 |
| BlinkBlox      | 160 |     125 to 168 |              1202 |          16382 | 0.0% |                                  0 |                45.8 |
| Zap            | 153 |     152 to 160 |              1203 |          16349 | 0.0% |                               4194 |                48.8 |
| Packet         | 142 |     140 to 152 |              1203 |          16224 | 0.0% |                                 66 |                57.3 |
| Roblox remotes | 136 |     128 to 144 |              2000 |         171186 | 0.0% |                                  0 |                29.1 |
| Lync           | 124 |     110 to 132 |              1202 |          16208 | 0.0% |                               1151 |                25.4 |
| Squash         |  89 |       48 to 96 |              1201 |          69156 | 0.0% |                               1208 |                54.6 |
| PakNet         |  74 |       71 to 90 |              1206 |          40259 | 0.0% |                               6058 |                37.1 |

**Numbers**

|                | FPS | FPS P10 to P90 | Bytes per message | KB/s at 60 FPS | Lost | Client bytes allocated per message | Server heap rise MB |
|----------------|-----|----------------|-------------------|----------------|------|------------------------------------|---------------------|
| **Courier**    | 143 |     128 to 148 |              1003 |          14837 | 0.0% |                                  0 |                51.3 |
| QuickNet       | 142 |     139 to 148 |              1003 |          14812 | 0.0% |                                 66 |                59.3 |
| Satset         | 134 |     132 to 144 |              1002 |          14746 | 0.0% |                                  0 |                37.6 |
| Blink          | 128 |     128 to 138 |              1003 |          14463 | 0.0% |                               3817 |                12.5 |
| BitNet         | 124 |     124 to 126 |              1003 |          15086 | 0.0% |                                  0 |                47.1 |
| Zap            | 118 |     112 to 124 |              1003 |          14952 | 0.0% |                               2097 |                21.5 |
| BlinkBlox      | 116 |     116 to 128 |              1003 |          15147 | 0.0% |                                  0 |                17.0 |
| NetRayCompile  | 114 |     112 to 116 |              1002 |          14987 | 0.0% |                                  0 |                60.9 |
| ByteNet        |  97 |      96 to 100 |              1003 |          14922 | 0.0% |                                  0 |                43.0 |
| Lync           |  90 |       87 to 92 |              1002 |          15120 | 0.0% |                                967 |                43.4 |
| Squash         |  80 |       79 to 83 |              1002 |         437691 | 0.0% |                               1012 |                35.9 |
| Packet         |  60 |       59 to 76 |              1003 |          15584 | 0.0% |                                 66 |                63.0 |
| PakNet         |  60 |       56 to 65 |              1006 |          78911 | 0.0% |                               3834 |                49.3 |
| Roblox remotes |  30 |       29 to 31 |             16000 |         367731 | 0.0% |                                  0 |                25.4 |

**Dict**

|                |         FPS | FPS P10 to P90 | Bytes per message | KB/s at 60 FPS | Lost | Client bytes allocated per message | Server heap rise MB |
|----------------|-------------|----------------|-------------------|----------------|------|------------------------------------|---------------------|
| **Courier**    |          55 |       54 to 58 |              1955 |          19959 | 0.0% |                                  0 |                59.1 |
| QuickNet       |          55 |       54 to 57 |              2010 |          21292 | 0.0% |                               3789 |                47.4 |
| Blink          |          53 |       50 to 54 |              2206 |          22866 | 0.0% |                               8618 |                55.9 |
| NetRayCompile  |          53 |       47 to 56 |              2005 |          20633 | 0.0% |                                  0 |                31.7 |
| BlinkBlox      |          52 |       52 to 53 |              2006 |          20575 | 0.0% |                                  0 |                36.5 |
| Zap            |          52 |       51 to 56 |              2007 |          20990 | 0.0% |                               4194 |                43.6 |
| BitNet         |          50 |       50 to 54 |              2006 |          20616 | 0.0% |                                  0 |                62.2 |
| Satset         |          39 |       38 to 41 |              2005 |          19957 | 0.0% |                               4145 |                58.0 |
| Squash         |          35 |       35 to 36 |              2005 |         347370 | 0.0% |                               2011 |                 3.4 |
| PakNet         |          34 |       31 to 38 |              2009 |         156118 | 0.0% |                               7373 |                21.1 |
| Lync           |          34 |       33 to 37 |              2006 |          21118 | 0.0% |                               2019 |                32.1 |
| Roblox remotes |          29 |       29 to 31 |              3203 |         131715 | 0.0% |                                  0 |                62.3 |
| ByteNet        | unsupported |                |                   |                |      |                                    |                     |
| Packet         | unsupported |                |                   |                |      |                                    |                     |

**Booleans**

|                | FPS | FPS P10 to P90 | Bytes per message | KB/s at 60 FPS | Lost | Client bytes allocated per message | Server heap rise MB |
|----------------|-----|----------------|-------------------|----------------|------|------------------------------------|---------------------|
| **Courier**    | 162 |     146 to 164 |               127 |           1929 | 0.0% |                                  0 |                31.1 |
| QuickNet       | 160 |     152 to 161 |               128 |           1870 | 0.0% |                                 66 |                50.7 |
| NetRayCompile  | 152 |     150 to 152 |               127 |           1896 | 0.0% |                                  0 |                23.3 |
| Satset         | 144 |     140 to 152 |               127 |           1887 | 0.0% |                                  0 |                66.4 |
| BlinkBlox      | 138 |     132 to 140 |               128 |           1825 | 0.0% |                                  0 |                58.4 |
| Blink          | 124 |     120 to 132 |              1003 |           2922 | 0.0% |                               3817 |                59.3 |
| Squash         | 116 |     116 to 124 |               127 |           3534 | 0.0% |                                135 |                32.3 |
| Lync           | 108 |     106 to 114 |               127 |           1872 | 0.0% |                                119 |                63.4 |
| Zap            | 106 |     100 to 110 |              1003 |           2885 | 0.0% |                               2097 |                25.5 |
| BitNet         |  87 |       84 to 89 |               128 |           1900 | 0.0% |                                  0 |                58.3 |
| ByteNet        |  81 |       76 to 92 |              1003 |           1558 | 0.0% |                                  0 |                45.5 |
| Packet         |  70 |       70 to 71 |              1003 |           2966 | 0.0% |                                 66 |                58.0 |
| PakNet         |  48 |       46 to 50 |               131 |           3974 | 0.0% |                               1475 |                41.3 |
| Roblox remotes |  31 |       30 to 32 |              9000 |          58717 | 0.0% |                                  0 |                60.1 |

**Entities**

|                | FPS | FPS P10 to P90 | Bytes per message | KB/s at 60 FPS | Lost | Client bytes allocated per message | Server heap rise MB |
|----------------|-----|----------------|-------------------|----------------|------|------------------------------------|---------------------|
| **Courier**    | 116 |     107 to 120 |               603 |           9044 | 0.0% |                                  0 |                63.4 |
| BlinkBlox      | 100 |      96 to 108 |               602 |           8746 | 0.0% |                                  0 |                68.4 |
| QuickNet       |  99 |      99 to 104 |               603 |           8800 | 0.0% |                                 66 |                63.5 |
| BitNet         |  81 |       79 to 85 |               602 |           9120 | 0.0% |                                  0 |                55.5 |
| Blink          |  80 |       79 to 81 |               603 |           9043 | 0.0% |                               2544 |                 7.7 |
| NetRayCompile  |  80 |       76 to 81 |               602 |           9087 | 0.0% |                                  0 |                47.7 |
| ByteNet        |  75 |       74 to 79 |               603 |           8922 | 0.0% |                                  0 |                58.2 |
| Zap            |  71 |       70 to 75 |               603 |           8287 | 0.0% |                               2097 |                55.5 |
| Lync           |  70 |       65 to 76 |               602 |           8749 | 0.0% |                                545 |                50.8 |
| Satset         |  60 |       58 to 60 |               602 |           8961 | 0.0% |                                  0 |                61.6 |
| Packet         |  53 |       52 to 59 |               603 |           9383 | 0.0% |                                 66 |                33.2 |
| PakNet         |  52 |       50 to 55 |               606 |          61708 | 0.0% |                               3432 |                33.0 |
| Squash         |  49 |       47 to 50 |               601 |         114951 | 0.0% |                              10207 |                40.2 |
| Roblox remotes |  18 |       18 to 18 |              8100 |         510830 | 0.0% |                                  0 |                35.8 |

**Strings**

|                |         FPS | FPS P10 to P90 | Bytes per message | KB/s at 60 FPS | Lost | Client bytes allocated per message | Server heap rise MB |
|----------------|-------------|----------------|-------------------|----------------|------|------------------------------------|---------------------|
| **Courier**    |          39 |       35 to 42 |              3229 |          27246 | 0.0% |                                  0 |                43.1 |
| Blink          |          36 |       35 to 41 |              3605 |          30758 | 0.0% |                              12923 |                14.1 |
| Zap            |          35 |       35 to 36 |              3305 |          30696 | 0.0% |                               8389 |                51.9 |
| NetRayCompile  |          35 |       34 to 38 |              3304 |          31675 | 0.0% |                                  0 |                35.5 |
| QuickNet       |          35 |       34 to 39 |              3309 |          33580 | 0.0% |                              10318 |                60.7 |
| BlinkBlox      |          34 |       34 to 40 |              3305 |          31033 | 0.0% |                                  0 |                49.2 |
| Packet         |          33 |       29 to 38 |              3305 |          31253 | 0.0% |                                 66 |                30.2 |
| Lync           |          32 |       31 to 38 |              3305 |          33274 | 0.0% |                               3318 |                54.6 |
| Satset         |          32 |       31 to 39 |              3604 |          32350 | 0.0% |                                  0 |                67.1 |
| BitNet         |          31 |       15 to 64 |              3305 |          29144 | 0.0% |                                  0 |                46.4 |
| Squash         |          28 |       28 to 29 |              3304 |         145677 | 0.0% |                               3310 |                24.8 |
| Roblox remotes |          26 |       24 to 27 |              5402 |         871869 | 0.0% |                                  0 |                39.3 |
| PakNet         |          25 |       25 to 30 |              3308 |         137777 | 0.0% |                              12259 |                33.9 |
| ByteNet        | unsupported |                |                   |                |      |                                    |                     |

**Memory held after the run**

|                | Server KB | Client KB |
|----------------|-----------|-----------|
| Roblox remotes |         8 |         8 |
| Zap            |       611 |        97 |
| Blink          |       589 |       156 |
| ByteNet        |       230 |       736 |
| PakNet         |       492 |       484 |
| BlinkBlox      |      1476 |       180 |
| NetRayCompile  |       400 |      1326 |
| Squash         |       871 |       869 |
| **Courier**    |       974 |       913 |
| Lync           |      2231 |      1826 |
| BitNet         |      1566 |      3095 |
| Packet         |      2678 |      2288 |
| Satset         |      2603 |      7657 |
| QuickNet       |      2695 |      9337 |

</details>

**Attack**, a client sending everything an exploiter could: junk values, random, cut and flipped
buffers, deep and sparse tables, missing and extra arguments, 8000 arguments, and broken JSON.

|                | Server errors | Warnings | Bad values handed to the game | Longest frame | Memory peak |
|----------------|---------------|----------|-------------------------------|---------------|-------------|
| **Courier**    |         **0** |    **0** |                         **0** |        317 ms |      +60 KB |
| Satset         |             0 |        0 |                             0 |        311 ms |       +3 KB |
| BitNet         |             0 |      894 |                             0 |        310 ms |        0 KB |
| Packet         |             0 |      317 |                             1 |        295 ms |      +82 KB |
| BlinkBlox      |             0 |        6 |                             2 |        317 ms |     +143 KB |
| Lync           |             0 |       90 |                             2 |        315 ms |       +4 KB |
| QuickNet       |             0 |      504 |                             6 |        548 ms |        0 KB |
| ByteNet        |            90 |        0 |                             1 |        323 ms |        0 KB |
| NetRayCompile  |           165 |        0 |                             4 |        310 ms |        0 KB |
| Squash         |           187 |        0 |                             8 |        565 ms |     +851 MB |
| Zap            |           420 |        0 |                             3 |        326 ms |       +3 KB |
| Blink          |           469 |        1 |                             2 |        311 ms |     +201 KB |
| PakNet         |           916 |        0 |                             2 |        316 ms |     +364 KB |
| Roblox remotes |             0 |        0 |                           919 |        301 ms |        0 KB |

Bad values include NaN vectors and broken tables that reached a handler. Memory
peak is the most the library's own scripts held over what they held before. Squash reads an array
length from the packet and allocates it before checking the bytes are there, so 826 KB of junk
buffers made it hold 851 MB.

<details>
<summary>Server errors / bad values by attack</summary>

|                |    Junk | Buffers | Truncated | Flipped | Overwritten |  Tables | Sparse | Arguments | Nil arguments | 8000 arguments |    JSON |
|----------------|---------|---------|-----------|---------|-------------|---------|--------|-----------|---------------|----------------|---------|
| **Courier**    |       - |       - |         - |       - |           - |       - |      - |         - |             - |              - |       - |
| Satset         |       - |       - |         - |       - |           - |       - |      - |         - |             - |              - |       - |
| BitNet         |       - |       - |         - |       - |           - |       - |      - |         - |             - |              - |       - |
| Packet         |       - |       - |         - |       - |       0 / 1 |       - |      - |         - |             - |              - |       - |
| BlinkBlox      |       - |       - |         - |       - |       0 / 2 |       - |      - |         - |             - |              - |       - |
| Lync           |       - |       - |         - |       - |       0 / 2 |       - |      - |         - |             - |              - |       - |
| QuickNet       |       - |       - |         - |   0 / 1 |       0 / 5 |       - |      - |         - |             - |              - |       - |
| ByteNet        |       - |  34 / 0 |    28 / 0 |   1 / 0 |      16 / 1 |       - |      - |    11 / 0 |             - |              - |       - |
| NetRayCompile  |  20 / 0 |  22 / 0 |    42 / 0 |  10 / 0 |       3 / 4 |   9 / 0 |      - |     7 / 0 |         7 / 0 |          9 / 0 |  36 / 0 |
| Squash         |       - |  91 / 0 |    29 / 4 |   7 / 0 |      28 / 4 |       - |      - |    32 / 0 |             - |              - |       - |
| Zap            |  20 / 0 |  19 / 0 |    42 / 0 |  11 / 0 |      30 / 3 |   9 / 0 |      - |    12 / 0 |        25 / 0 |              - | 252 / 0 |
| Blink          |  39 / 0 |   9 / 0 |    42 / 0 |  16 / 0 |      17 / 2 |  15 / 0 |      - |    11 / 0 |        32 / 0 |              - | 288 / 0 |
| PakNet         | 120 / 0 | 120 / 0 |    48 / 0 |  12 / 0 |      28 / 2 |  54 / 0 |      - |    42 / 0 |        60 / 0 |              - | 432 / 0 |
| Roblox remotes | 0 / 120 | 0 / 120 |         - |       - |           - | 0 / 145 | 0 / 14 |    0 / 42 |        0 / 60 |              - | 0 / 418 |

</details>

## License

This project has the [MIT License](https://github.com/XoifaiI/Courier/blob/main/LICENSE).
