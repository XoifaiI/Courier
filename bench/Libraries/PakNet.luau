--!strict

local PakNet = require("../Vendor/paknet") :: any

local TESTS = { "Booleans", "Entities", "Strings", "Numbers", "Dict", "Vectors" }

local Byte = PakNet.UInt8
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
})

local Fire: { [string]: (any) -> () } = {}
for _, Test in TESTS do
	local Target = Net[Test]
	Fire[Test] = function(Value: any)
		Target:FireServer(Value)
	end
end

return {
	Tests = TESTS,
	Fire = Fire,
	On = function(Test: string, Handler: (Player, any) -> ())
		Net[Test].OnServerEvent:Connect(Handler)
	end,
}
