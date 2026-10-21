--!strict

local RunService = game:GetService("RunService")

local BlinkBlox = if RunService:IsServer()
	then require("../../Vendor/blinkblox/Server")
	else require("../../Vendor/blinkblox/Client")
local Benches = require("../Benches")

local Net = BlinkBlox :: any

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
