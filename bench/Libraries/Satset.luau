--!strict

local Satset = require("../Vendor/satset") :: any

local TESTS = { "Booleans", "Dict", "Entities", "Numbers", "Strings", "Vectors" }

local Types = Satset.Types

local WRAPPED: { [string]: boolean } = { State = true, Chat = true }

local function Define(Name: string, Schema: any): any
	return Satset.definePacket({ name = Name, schema = Schema, reliable = true })
end

Satset.start()

local Packets: { [string]: any } = {
	Booleans = Define("Booleans", { Value = Types.array(Types.bool) }),
	Entities = Define("Entities", {
		Value = Types.array(Satset.struct({
			id = Types.u8,
			x = Types.u8,
			y = Types.u8,
			z = Types.u8,
			orientation = Types.u8,
			animation = Types.u8,
		})),
	}),
	Strings = Define("Strings", { Value = Types.array(Types.string8) }),
	Numbers = Define("Numbers", { Value = Types.array(Types.u8) }),
	Dict = Define("Dict", { Value = Types.map(Types.string8, Types.u8) }),
	Vectors = Define("Vectors", { Value = Types.array(Types.Vector3) }),
}

local GamePackets: { [string]: any } = {
	State = Define("State", {
		Value = Types.array(
			Satset.struct({ id = Types.u16, position = Types.Vector3, yaw = Types.u16, health = Types.u8 })
		),
	}),
	Hit = Define("Hit", { attacker = Types.u16, target = Types.u16, damage = Types.u8 }),
	Pickup = Define("Pickup", { item = Types.u16, slot = Types.u8 }),
	Chat = Define("Chat", { Value = Types.string8 }),
	Input = Define("Input", {
		move = Types.Vector3,
		look = Types.Vector3,
		jump = Types.bool,
		fire = Types.bool,
		crouch = Types.bool,
	}),
}

local Fire: { [string]: (any) -> () } = {}
for Test, Target in Packets do
	local Payload: { Value: any } = { Value = nil }
	Fire[Test] = function(Value: any)
		Payload.Value = Value
		Target:fireServer(Payload)
	end
end

local Send: { [string]: (Player, any) -> () } = {}
for _, Name in { "State", "Hit", "Pickup", "Chat" } do
	local Target = GamePackets[Name]
	if WRAPPED[Name] then
		local Payload: { Value: any } = { Value = nil }
		Send[Name] = function(Player: Player, Value: any)
			Payload.Value = Value
			Target:fireClient(Player, Payload)
		end
	else
		Send[Name] = function(Player: Player, Value: any)
			Target:fireClient(Player, Value)
		end
	end
end

return {
	Tests = TESTS,
	Fire = Fire,
	On = function(Test: string, Handler: (Player, any) -> ())
		Packets[Test]:listenServer(function(Player: Player, Payload: any)
			Handler(Player, Payload.Value)
		end)
	end,
	Game = {
		Send = Send,
		On = function(Name: string, Handler: (any) -> ())
			GamePackets[Name]:listen(function(Payload: any)
				Handler(if WRAPPED[Name] then Payload.Value else Payload)
			end)
		end,
		Input = function(Value: any)
			GamePackets.Input:fireServer(Value)
		end,
		OnInput = function(Handler: (Player, any) -> ())
			GamePackets.Input:listenServer(Handler)
		end,
	},
}
