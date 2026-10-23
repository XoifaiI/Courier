--!strict

local Packet = require("../Vendor/packet") :: any

local TESTS = { "Booleans", "Entities", "Numbers", "Strings", "Vectors" }

local Byte = Packet.NumberU8
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

local Fire: { [string]: (any) -> () } = {}
for Test, Target in Events do
	Fire[Test] = function(Value: any)
		Target:Fire(Value)
	end
end

return {
	Tests = TESTS,
	Fire = Fire,
	On = function(Test: string, Handler: (Player, any) -> ())
		Events[Test].OnServerEvent:Connect(Handler)
	end,
}
