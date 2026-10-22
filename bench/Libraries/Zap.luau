--!strict

local RunService = game:GetService("RunService")

local TESTS = { "Booleans", "Dict", "Entities", "Numbers", "Strings", "Vectors" }

local Net = (if RunService:IsServer() then require("../Vendor/zap/Server") else require("../Vendor/zap/Client")) :: any

local Fire: { [string]: (any) -> () } = {}
for _, Test in TESTS do
	Fire[Test] = Net[Test].Fire
end

return {
	Tests = TESTS,
	Fire = Fire,
	On = function(Test: string, Handler: (Player, any) -> ())
		Net[Test].SetCallback(Handler)
	end,
	Game = {
		Send = { State = Net.State.Fire, Hit = Net.Hit.Fire, Pickup = Net.Pickup.Fire, Chat = Net.Chat.Fire },
		On = function(Name: string, Handler: (any) -> ())
			Net[Name].SetCallback(Handler)
		end,
		Input = Net.Input.Fire,
		OnInput = function(Handler: (Player, any) -> ())
			Net.Input.SetCallback(Handler)
		end,
	},
}
