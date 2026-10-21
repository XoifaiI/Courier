--!strict

return function(QuickNet: any)
	local Data = QuickNet.Data

	local Events = {
		Booleans = QuickNet:register("BoolTest", { Data.Boolean }):SetRateLimit(1000000000, 30),
		Entities = QuickNet:register("EntitiesTest", {
			{
				id = Data.NumberU8,
				x = Data.NumberU8,
				y = Data.NumberU8,
				z = Data.NumberU8,
				orientation = Data.NumberU8,
				animation = Data.NumberU8,
			},
		}):SetRateLimit(1000000000, 30),
		Strings = QuickNet:register("StringsTest", { Data.String }):SetRateLimit(1000000000, 30),
		Numbers = QuickNet:register("NumbersTest", { Data.NumberU8 }):SetRateLimit(1000000000, 30),
		Dict = QuickNet:register("DictTest", { [Data.String] = Data.NumberU8 }):SetRateLimit(1000000000, 30),
		Vectors = QuickNet:register("VectorsTest", { Data.Vector3F32 }):SetRateLimit(100000000000, 30),
	}

	local GameEvents = {
		State = QuickNet:register("GameState", {
			{ id = Data.NumberU16, position = Data.Vector3F32, yaw = Data.NumberU16, health = Data.NumberU8 },
		}):SetRateLimit(1000000000, 30),
		Hit = QuickNet:register("GameHit", {
			attacker = Data.NumberU16,
			target = Data.NumberU16,
			damage = Data.NumberU8,
		}):SetRateLimit(1000000000, 30),
		Pickup = QuickNet:register("GamePickup", { item = Data.NumberU16, slot = Data.NumberU8 })
			:SetRateLimit(1000000000, 30),
		Chat = QuickNet:register("GameChat", Data.String):SetRateLimit(1000000000, 30),
		Input = QuickNet:register("GameInput", {
			move = Data.Vector3F32,
			look = Data.Vector3F32,
			jump = Data.Boolean,
			fire = Data.Boolean,
			crouch = Data.Boolean,
		}):SetRateLimit(1000000000, 30),
	}

	local Fire: { [string]: (any) -> () } = {}
	for Name, Event in Events do
		Fire[Name] = function(Value: any)
			Event:FireServer(Value)
		end
	end

	local Send: { [string]: (Player, any) -> () } = {}
	for _, Name in { "State", "Hit", "Pickup", "Chat" } do
		local Event = GameEvents[Name]
		Send[Name] = function(Player: Player, Value: any)
			Event:FireClient(Player, Value)
		end
	end

	local Input = GameEvents.Input

	return {
		Fire = Fire,
		On = function(Name: string, Callback: (Player, any) -> ())
			Events[Name].OnServerEvent:Connect(Callback)
		end,
		Game = {
			Send = Send,
			On = function(Name: string, Callback: (any) -> ())
				GameEvents[Name].OnClientEvent:Connect(Callback)
			end,
			Fire = {
				Input = function(Value: any)
					Input:FireServer(Value)
				end,
			},
			Listen = function(Name: string, Callback: (Player, any) -> ())
				GameEvents[Name].OnServerEvent:Connect(Callback)
			end,
		},
	}
end
