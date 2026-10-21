--!strict

return function(Packet: any)
	local Byte = Packet.NumberU8
	local Short = Packet.NumberU16
	local Vector = Packet.Vector3F32
	local Flag = Packet.Boolean8

	local Events: { [string]: any } = {
		Booleans = Packet("Booleans", { Packet.Boolean8 }),
		Entities = Packet("Entities", {
			{ id = Byte, x = Byte, y = Byte, z = Byte, orientation = Byte, animation = Byte },
		}),
		Strings = Packet("Strings", { Packet.String }),
		Numbers = Packet("Numbers", { Byte }),
		Dict = Packet("Dict", Packet.Any),
		Vectors = Packet("Vectors", { Packet.Vector3F32 }),
	}

	local GameEvents: { [string]: any } = {
		State = Packet("State", { { id = Short, position = Vector, yaw = Short, health = Byte } }),
		Hit = Packet("Hit", { attacker = Short, target = Short, damage = Byte }),
		Pickup = Packet("Pickup", { item = Short, slot = Byte }),
		Chat = Packet("Chat", Packet.String),
		Input = Packet("Input", { move = Vector, look = Vector, jump = Flag, fire = Flag, crouch = Flag }),
	}

	local Fire: { [string]: (any) -> () } = {}
	for Name, Event in Events do
		Fire[Name] = function(Value: any)
			Event:Fire(Value)
		end
	end

	local State, Hit, Pickup, Chat, Input =
		GameEvents.State, GameEvents.Hit, GameEvents.Pickup, GameEvents.Chat, GameEvents.Input

	return {
		Fire = Fire,
		On = function(Name: string, Callback: (Player, any) -> ())
			Events[Name].OnServerEvent:Connect(Callback)
		end,
		Game = {
			Send = {
				State = function(Player: Player, Data: any)
					State:FireClient(Player, Data)
				end,
				Hit = function(Player: Player, Data: any)
					Hit:FireClient(Player, Data)
				end,
				Pickup = function(Player: Player, Data: any)
					Pickup:FireClient(Player, Data)
				end,
				Chat = function(Player: Player, Data: any)
					Chat:FireClient(Player, Data)
				end,
			},
			On = function(Name: string, Callback: (any) -> ())
				GameEvents[Name].OnClientEvent:Connect(Callback)
			end,
			Fire = {
				Input = function(Data: any)
					Input:Fire(Data)
				end,
			},
			Listen = function(Name: string, Callback: (Player, any) -> ())
				GameEvents[Name].OnServerEvent:Connect(Callback)
			end,
		},
	}
end
