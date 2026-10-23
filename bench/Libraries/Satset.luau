--!strict

local Satset = require("../Vendor/satset") :: any

local TESTS = { "Booleans", "Dict", "Entities", "Numbers", "Strings", "Vectors" }

local Types = Satset.Types

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

local Fire: { [string]: (any) -> () } = {}
for Test, Target in Packets do
	local Payload: { Value: any } = { Value = nil }
	Fire[Test] = function(Value: any)
		Payload.Value = Value
		Target:fireServer(Payload)
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
}
