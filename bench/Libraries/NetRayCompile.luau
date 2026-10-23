--!strict

local RunService = game:GetService("RunService")

local TESTS = { "Booleans", "Dict", "Entities", "Numbers", "Strings", "Vectors" }

local Net = (
	if RunService:IsServer()
		then require("../Vendor/netraycompile/Server")
		else require("../Vendor/netraycompile/Client")
) :: any

local Fire: { [string]: (any) -> () } = {}
for _, Test in TESTS do
	Fire[Test] = Net[Test].Fire
end

return {
	Tests = TESTS,
	Fire = Fire,
	On = function(Test: string, Handler: (Player, any) -> ())
		Net[Test].On(Handler)
	end,
}
