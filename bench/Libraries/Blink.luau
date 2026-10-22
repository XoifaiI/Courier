--!strict

local RunService = game:GetService("RunService")

local TESTS = { "Booleans", "Dict", "Entities", "Numbers", "Strings", "Vectors" }

local Net = (
	if RunService:IsServer() then require("../Vendor/blink/Server") else require("../Vendor/blink/Client")
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
	Game = {
		Send = { State = Net.State.Fire, Hit = Net.Hit.Fire, Pickup = Net.Pickup.Fire, Chat = Net.Chat.Fire },
		On = function(Name: string, Handler: (any) -> ())
			Net[Name].On(Handler)
		end,
		Input = Net.Input.Fire,
		OnInput = function(Handler: (Player, any) -> ())
			Net.Input.On(Handler)
		end,
	},
}
