--!strict

local BitNet = (require("../../Vendor/bitnet") :: any)({ VALIDATE_OUTGOING_TYPES = false })
local Types = BitNet.types

local Byte = Types.u8

local Net: { [string]: any } = {
	Booleans = BitNet.event({ Args = Types.array(Types.bool) }),
	Entities = BitNet.event({
		Args = Types.array(
			Types.struct({ id = Byte, x = Byte, y = Byte, z = Byte, orientation = Byte, animation = Byte })
		),
	}),
	Strings = BitNet.event({ Args = Types.array(Types.string) }),
	Numbers = BitNet.event({ Args = Types.array(Byte) }),
	Dict = BitNet.event({ Args = Types.map(Types.string, Byte) }),
	Vectors = BitNet.event({ Args = Types.array(Types.vec3) }),
}

local GameNet: { [string]: any } = {}
GameNet.State = BitNet.event({
	Args = Types.array(Types.struct({ id = Types.u16, position = Types.vec3, yaw = Types.u16, health = Byte })),
})
GameNet.Hit = BitNet.event({ Args = Types.struct({ attacker = Types.u16, target = Types.u16, damage = Byte }) })
GameNet.Pickup = BitNet.event({ Args = Types.struct({ item = Types.u16, slot = Byte }) })
GameNet.Chat = BitNet.event({ Args = Types.string })
GameNet.Input = BitNet.event({
	Args = Types.struct({
		move = Types.vec3,
		look = Types.vec3,
		jump = Types.bool,
		fire = Types.bool,
		crouch = Types.bool,
	}),
})

local Send: { [string]: (Player, any) -> () } = {}
for _, Name in { "State", "Hit", "Pickup", "Chat" } do
	local Event = GameNet[Name]
	Send[Name] = function(Player: Player, Data: any)
		Event:FireClient(Player, Data)
	end
end

local Input = GameNet.Input

local Benches = require("../Benches")

local Fire: { [string]: (any) -> () } = {}
for Name in Benches do
	local Event = Net[Name]
	Fire[Name] = function(Data: any)
		Event:FireServer(Data)
	end
end

return {
	Fire = Fire,
	On = function(Name: string, Callback: (Player, any) -> ())
		Net[Name].OnServerEvent:Connect(Callback)
	end,
	Game = {
		Send = Send,
		On = function(Name: string, Callback: (any) -> ())
			GameNet[Name].OnClientEvent:Connect(Callback)
		end,
		Fire = {
			Input = function(Data: any)
				Input:FireServer(Data)
			end,
		},
		Listen = function(Name: string, Callback: (Player, any) -> ())
			GameNet[Name].OnServerEvent:Connect(Callback)
		end,
	},
}
