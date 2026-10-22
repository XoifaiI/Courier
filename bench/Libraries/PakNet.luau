--!strict

local PakNet = require("../Vendor/paknet") :: any

local TESTS = { "Booleans", "Entities", "Strings", "Numbers", "Dict", "Vectors" }

local Byte = PakNet.UInt8
local Short = PakNet.UInt16
local Vector = PakNet.Vector3()

local function Remote(Datatype: any): any
	return PakNet:DefineRemote({ params = PakNet:Schema(Datatype), remoteType = "r" })
end

local Net = PakNet:Mount(script, {
	Booleans = Remote(PakNet.Array(PakNet.Boolean)),
	Entities = Remote(
		PakNet.Array(
			PakNet.Dictionary({ id = Byte, x = Byte, y = Byte, z = Byte, orientation = Byte, animation = Byte })
		)
	),
	Strings = Remote(PakNet.Array(PakNet.String8)),
	Numbers = Remote(PakNet.Array(Byte)),
	Dict = Remote(PakNet.Map(PakNet.String8, Byte)),
	Vectors = Remote(PakNet.Array(Vector)),
	State = Remote(
		PakNet.Array(PakNet.Dictionary({ id = Short, position = Vector, yaw = Short, health = Byte }), Byte)
	),
	Hit = Remote(PakNet.Dictionary({ attacker = Short, target = Short, damage = Byte })),
	Pickup = Remote(PakNet.Dictionary({ item = Short, slot = Byte })),
	Chat = Remote(PakNet.String8),
	Input = Remote(PakNet.Dictionary({
		move = Vector,
		look = Vector,
		jump = PakNet.Boolean,
		fire = PakNet.Boolean,
		crouch = PakNet.Boolean,
	})),
})

local Fire: { [string]: (any) -> () } = {}
for _, Test in TESTS do
	local Target = Net[Test]
	Fire[Test] = function(Value: any)
		Target:FireServer(Value)
	end
end

local Send: { [string]: (Player, any) -> () } = {}
for _, Name in { "State", "Hit", "Pickup", "Chat" } do
	local Target = Net[Name]
	Send[Name] = function(Player: Player, Value: any)
		Target:Fire(Player, Value)
	end
end

return {
	Tests = TESTS,
	Fire = Fire,
	On = function(Test: string, Handler: (Player, any) -> ())
		Net[Test].OnServerEvent:Connect(Handler)
	end,
	Game = {
		Send = Send,
		On = function(Name: string, Handler: (any) -> ())
			Net[Name].OnClientEvent:Connect(Handler)
		end,
		Input = function(Value: any)
			Net.Input:FireServer(Value)
		end,
		OnInput = function(Handler: (Player, any) -> ())
			Net.Input.OnServerEvent:Connect(Handler)
		end,
	},
}
