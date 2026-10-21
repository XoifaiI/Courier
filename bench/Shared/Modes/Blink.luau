--!strict

local RunService = game:GetService("RunService")

local Blink = if RunService:IsServer()
	then require("../../Vendor/blink/Server")
	else require("../../Vendor/blink/Client")
local Benches = require("../Benches")

local Net = Blink :: any

local Fire: { [string]: (any) -> () } = {}
for Name in Benches do
	Fire[Name] = Net[Name].Fire
end

return {
	Fire = Fire,
	On = function(Name: string, Callback: (Player, any) -> ())
		Net[Name].On(Callback)
	end,
	Game = {
		Send = { State = Net.State.Fire, Hit = Net.Hit.Fire, Pickup = Net.Pickup.Fire, Chat = Net.Chat.Fire },
		On = function(Name: string, Callback: (any) -> ())
			Net[Name].On(Callback)
		end,
		Fire = { Input = Net.Input.Fire },
		Listen = function(Name: string, Callback: (Player, any) -> ())
			Net[Name].On(Callback)
		end,
	},
}
