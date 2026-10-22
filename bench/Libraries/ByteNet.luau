--!strict

local ReplicatedStorage = game:GetService("ReplicatedStorage")
local RunService = game:GetService("RunService")

local ByteNet = require("../Vendor/bytenet") :: any

local TESTS = { "Booleans", "Entities", "Numbers", "Vectors" }

if not RunService:IsServer() then
	ReplicatedStorage:WaitForChild("BytenetStorage"):WaitForChild("Bench")
end

local Net = ByteNet.defineNamespace("Bench", function()
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
		Numbers = ByteNet.definePacket({ value = ByteNet.array(ByteNet.uint8) }),
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
for _, Test in TESTS do
	Fire[Test] = Net[Test].send
end

local Send: { [string]: (Player, any) -> () } = {}
for _, Name in { "State", "Hit", "Pickup", "Chat" } do
	local SendTo = Net[Name].sendTo
	Send[Name] = function(Player: Player, Value: any)
		SendTo(Value, Player)
	end
end

return {
	Tests = TESTS,
	Fire = Fire,
	On = function(Test: string, Handler: (Player, any) -> ())
		Net[Test].listen(function(Value: any, Player: Player)
			Handler(Player, Value)
		end)
	end,
	Game = {
		Send = Send,
		On = function(Name: string, Handler: (any) -> ())
			Net[Name].listen(Handler)
		end,
		Input = Net.Input.send,
		OnInput = function(Handler: (Player, any) -> ())
			Net.Input.listen(function(Value: any, Player: Player)
				Handler(Player, Value)
			end)
		end,
	},
}
