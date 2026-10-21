--!strict

local RunService = game:GetService("RunService")

local NetRay = if RunService:IsServer()
	then require("../../Vendor/netraycompile/Server")
	else require("../../Vendor/netraycompile/Client")
local Benches = require("../Benches")

local Net = NetRay :: any

local Fire: { [string]: (any) -> () } = {}
for Name in Benches do
	Fire[Name] = Net[Name].Fire
end

local Receivers: { [string]: ((any) -> ()) -> () } = {
	State = function(Callback: (any) -> ())
		Net.State.On(Callback)
	end,
	Hit = function(Callback: (any) -> ())
		Net.Hit.On(function(Attacker: number, Target: number, Damage: number)
			Callback({ attacker = Attacker, target = Target, damage = Damage })
		end)
	end,
	Pickup = function(Callback: (any) -> ())
		Net.Pickup.On(function(Item: number, Slot: number)
			Callback({ item = Item, slot = Slot })
		end)
	end,
	Chat = function(Callback: (any) -> ())
		Net.Chat.On(Callback)
	end,
}

return {
	Fire = Fire,
	On = function(Name: string, Callback: (Player, any) -> ())
		Net[Name].On(Callback)
	end,
	Game = {
		Send = { State = Net.State.Fire, Hit = Net.Hit.Fire, Pickup = Net.Pickup.Fire, Chat = Net.Chat.Fire },
		On = function(Name: string, Callback: (any) -> ())
			Receivers[Name](Callback)
		end,
		Fire = { Input = Net.Input.Fire },
		Listen = function(_: string, Callback: (Player, any) -> ())
			Net.Input.On(
				function(Player: Player, Move: Vector3, Look: Vector3, Jump: boolean, Shoot: boolean, Crouch: boolean)
					Callback(Player, { move = Move, look = Look, jump = Jump, fire = Shoot, crouch = Crouch })
				end
			)
		end,
	},
}
