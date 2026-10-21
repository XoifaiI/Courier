--!strict

local RunService = game:GetService("RunService")

local Zap = (
	if RunService:IsServer() then require("../../Vendor/zap/Server") else require("../../Vendor/zap/Client")
) :: any
local Benches = require("../Benches")

local Fire: { [string]: (any) -> () } = {}
for Name in Benches do
	Fire[Name] = Zap[Name].Fire
end

return {
	Fire = Fire,
	On = function(Name: string, Callback: (Player, any) -> ())
		Zap[Name].SetCallback(Callback)
	end,
	Game = {
		Send = { State = Zap.State.Fire, Hit = Zap.Hit.Fire, Pickup = Zap.Pickup.Fire, Chat = Zap.Chat.Fire },
		On = function(Name: string, Callback: (any) -> ())
			Zap[Name].SetCallback(Callback)
		end,
		Fire = { Input = Zap.Input.Fire },
		Listen = function(Name: string, Callback: (Player, any) -> ())
			Zap[Name].SetCallback(Callback)
		end,
	},
}
