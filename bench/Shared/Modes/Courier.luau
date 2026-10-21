--!strict

local ReplicatedStorage = game:GetService("ReplicatedStorage")

local Config = (require(ReplicatedStorage:WaitForChild("Courier")) :: any).Config
Config.MaxFrameBytes = 16777216
Config.ReliableCapacity = (require("../../Config") :: any).CourierCapacity

local Net = require("../../Vendor/net") :: any
local Benches = require("../Benches")

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
