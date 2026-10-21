--!strict

local ReplicatedStorage = game:GetService("ReplicatedStorage")
local RunService = game:GetService("RunService")

local ByteNet = require("../../Vendor/bytenet") :: any
local Benches = require("../Benches")

if not RunService:IsServer() then
	ReplicatedStorage:WaitForChild("BytenetStorage"):WaitForChild("RivalBench")
end

local Net = ByteNet.defineNamespace("RivalBench", function()
	local Entity = ByteNet.struct({
		id = ByteNet.uint8,
		x = ByteNet.uint8,
		y = ByteNet.uint8,
		z = ByteNet.uint8,
		orientation = ByteNet.uint8,
		animation = ByteNet.uint8,
	})

	local Unit = ByteNet.struct({
		id = ByteNet.uint16,
		position = ByteNet.vec3,
		yaw = ByteNet.uint16,
		health = ByteNet.uint8,
	})

	return {
		Booleans = ByteNet.definePacket({ value = ByteNet.array(ByteNet.bool) }),
		Entities = ByteNet.definePacket({ value = ByteNet.array(Entity) }),
		Strings = ByteNet.definePacket({ value = ByteNet.array(ByteNet.string) }),
		Numbers = ByteNet.definePacket({ value = ByteNet.array(ByteNet.uint8) }),
		Dict = ByteNet.definePacket({ value = ByteNet.map(ByteNet.string, ByteNet.uint8) }),
		Vectors = ByteNet.definePacket({ value = ByteNet.array(ByteNet.vec3) }),
		State = ByteNet.definePacket({ value = ByteNet.array(Unit) }),
		Hit = ByteNet.definePacket({
			value = ByteNet.struct({ attacker = ByteNet.uint16, target = ByteNet.uint16, damage = ByteNet.uint8 }),
		}),
		Pickup = ByteNet.definePacket({ value = ByteNet.struct({ item = ByteNet.uint16, slot = ByteNet.uint8 }) }),
		Chat = ByteNet.definePacket({ value = ByteNet.string }),
		Input = ByteNet.definePacket({
			value = ByteNet.struct({
				move = ByteNet.vec3,
				look = ByteNet.vec3,
				jump = ByteNet.bool,
				fire = ByteNet.bool,
				crouch = ByteNet.bool,
			}),
		}),
	}
end)

local Fire: { [string]: (any) -> () } = {}
for Name in Benches do
	Fire[Name] = Net[Name].send
end

local function Sender(Name: string): (Player, any) -> ()
	local SendTo = Net[Name].sendTo
	return function(Player: Player, Data: any)
		SendTo(Data, Player)
	end
end

return {
	Skip = { Strings = true, Dict = true },
	Fire = Fire,
	On = function(Name: string, Callback: (Player, any) -> ())
		Net[Name].listen(function(Data: any, Player: Player)
			Callback(Player, Data)
		end)
	end,
	Game = {
		Send = { State = Sender("State"), Hit = Sender("Hit"), Pickup = Sender("Pickup"), Chat = Sender("Chat") },
		On = function(Name: string, Callback: (any) -> ())
			Net[Name].listen(Callback)
		end,
		Fire = { Input = Net.Input.send },
		Listen = function(Name: string, Callback: (Player, any) -> ())
			Net[Name].listen(function(Data: any, Player: Player)
				Callback(Player, Data)
			end)
		end,
	},
}
