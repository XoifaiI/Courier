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
	return {
		Booleans = ByteNet.definePacket({ value = ByteNet.array(ByteNet.bool) }),
		Entities = ByteNet.definePacket({ value = ByteNet.array(Entity) }),
		Numbers = ByteNet.definePacket({ value = ByteNet.array(ByteNet.uint8) }),
		Vectors = ByteNet.definePacket({ value = ByteNet.array(ByteNet.vec3) }),
	}
end)

local Fire: { [string]: (any) -> () } = {}
for _, Test in TESTS do
	Fire[Test] = Net[Test].send
end

return {
	Tests = TESTS,
	Fire = Fire,
	On = function(Test: string, Handler: (Player, any) -> ())
		Net[Test].listen(function(Value: any, Player: Player)
			Handler(Player, Value)
		end)
	end,
}
