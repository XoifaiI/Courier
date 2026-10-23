--!strict

local RunService = game:GetService("RunService")

local Lync = require("../Vendor/lync") :: any

local TESTS = { "Booleans", "Entities", "Strings", "Numbers", "Dict", "Vectors" }

local Byte = Lync.int(0, 255)

local Net = Lync.define("bench", {
	Booleans = Lync.packet(Lync.array(Lync.bool(), 0, 1000)),
	Entities = Lync.packet(
		Lync.array(
			Lync.struct({ id = Byte, x = Byte, y = Byte, z = Byte, orientation = Byte, animation = Byte }),
			0,
			1000
		)
	),
	Strings = Lync.packet(Lync.array(Lync.str(0, 50), 0, 500)),
	Numbers = Lync.packet(Lync.array(Byte, 0, 2000)),
	Dict = Lync.packet(Lync.map(Lync.str(0, 50), Byte, 0, 500)),
	Vectors = Lync.packet(Lync.array(Lync.vec3(), 0, 500)),
})

Lync.start()
RunService.PostSimulation:Connect(function()
	Lync.flush()
end)

local Fire: { [string]: (any) -> () } = {}
for _, Test in TESTS do
	local Target = Net[Test]
	Fire[Test] = function(Value: any)
		Target:fireServer(Value)
	end
end

return {
	Tests = TESTS,
	Fire = Fire,
	On = function(Test: string, Handler: (Player, any) -> ())
		Net[Test]:onServer(function(Value: any, Player: Player)
			Handler(Player, Value)
		end)
	end,
}
