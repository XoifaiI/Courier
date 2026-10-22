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

local RECEIVERS: { [string]: ((any) -> ()) -> () } = {
	State = function(Handler: (any) -> ())
		Net.State.On(Handler)
	end,
	Hit = function(Handler: (any) -> ())
		Net.Hit.On(function(Attacker: number, Target: number, Damage: number)
			Handler({ attacker = Attacker, target = Target, damage = Damage })
		end)
	end,
	Pickup = function(Handler: (any) -> ())
		Net.Pickup.On(function(Item: number, Slot: number)
			Handler({ item = Item, slot = Slot })
		end)
	end,
	Chat = function(Handler: (any) -> ())
		Net.Chat.On(Handler)
	end,
}

return {
	Tests = TESTS,
	Fire = Fire,
	On = function(Test: string, Handler: (Player, any) -> ())
		Net[Test].On(Handler)
	end,
	Game = {
		Send = { State = Net.State.Fire, Hit = Net.Hit.Fire, Pickup = Net.Pickup.Fire, Chat = Net.Chat.Fire },
		On = function(Name: string, Handler: (any) -> ())
			RECEIVERS[Name](Handler)
		end,
		Input = Net.Input.Fire,
		OnInput = function(Handler: (Player, any) -> ())
			Net.Input.On(
				function(Player: Player, Move: Vector3, Look: Vector3, Jump: boolean, Shoot: boolean, Crouch: boolean)
					Handler(Player, { move = Move, look = Look, jump = Jump, fire = Shoot, crouch = Crouch })
				end
			)
		end,
	},
}
