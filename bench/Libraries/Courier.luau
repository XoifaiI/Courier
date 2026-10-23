--!strict

local ReplicatedStorage = game:GetService("ReplicatedStorage")

local TESTS = { "Booleans", "Dict", "Entities", "Numbers", "Strings", "Vectors" }

local Config = (require(ReplicatedStorage:WaitForChild("Courier")) :: any).Config
Config.MaxFrameBytes = 16777216

local Net = require("../Vendor/net") :: any

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
