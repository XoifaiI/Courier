--!strict

local Packet = require("../Vendor/packet") :: any

local TESTS = { "Booleans", "Entities", "Numbers", "Strings", "Vectors" }

local Byte = Packet.NumberU8
local Short = Packet.NumberU16
local Vector = Packet.Vector3F32
local Flag = Packet.Boolean8

local Events: { [string]: any } = {
	Booleans = Packet("Booleans", { Flag }),
	Entities = Packet(
		"Entities",
		{ { id = Byte, x = Byte, y = Byte, z = Byte, orientation = Byte, animation = Byte } }
	),
	Strings = Packet("Strings", { Packet.String }),
	Numbers = Packet("Numbers", { Byte }),
	Vectors = Packet("Vectors", { Vector }),
}

local GameEvents: { [string]: any } = {
	State = Packet("State", { { id = Short, position = Vector, yaw = Short, health = Byte } }),
	Hit = Packet("Hit", { attacker = Short, target = Short, damage = Byte }),
	Pickup = Packet("Pickup", { item = Short, slot = Byte }),
	Chat = Packet("Chat", Packet.String),
	Input = Packet("Input", { move = Vector, look = Vector, jump = Flag, fire = Flag, crouch = Flag }),
}

local Fire: { [string]: (any) -> () } = {}
for Test, Target in Events do
	Fire[Test] = function(Value: any)
		Target:Fire(Value)
	end
end

local Send: { [string]: (Player, any) -> () } = {}
for _, Name in { "State", "Hit", "Pickup", "Chat" } do
	local Target = GameEvents[Name]
	Send[Name] = function(Player: Player, Value: any)
		Target:FireClient(Player, Value)
	end
end

return {
	Tests = TESTS,
	Fire = Fire,
	On = function(Test: string, Handler: (Player, any) -> ())
		Events[Test].OnServerEvent:Connect(Handler)
	end,
	Game = {
		Send = Send,
		On = function(Name: string, Handler: (any) -> ())
			GameEvents[Name].OnClientEvent:Connect(Handler)
		end,
		Input = function(Value: any)
			GameEvents.Input:Fire(Value)
		end,
		OnInput = function(Handler: (Player, any) -> ())
			GameEvents.Input.OnServerEvent:Connect(Handler)
		end,
	},
}
