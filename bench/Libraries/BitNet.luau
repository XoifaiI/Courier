--!strict

local BitNet = (require("../Vendor/bitnet") :: any)({})

local TESTS = { "Booleans", "Dict", "Entities", "Numbers", "Strings", "Vectors" }

local Types = BitNet.types
local Byte = Types.u8

local Events: { [string]: any } = {
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

local Fire: { [string]: (any) -> () } = {}
for Test, Target in Events do
	Fire[Test] = function(Value: any)
		Target:FireServer(Value)
	end
end

return {
	Tests = TESTS,
	Fire = Fire,
	On = function(Test: string, Handler: (Player, any) -> ())
		Events[Test].OnServerEvent:Connect(Handler)
	end,
}
