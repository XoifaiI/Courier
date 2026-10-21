--!strict

local RunService = game:GetService("RunService")

local Lync = require("../../Vendor/lync") :: any

local Byte = Lync.int(0, 255)
local Short = Lync.int(0, 65535)

local Net = Lync.define("bench", {
	Booleans = Lync.packet(Lync.array(Lync.bool(), 0, 1000)),
	Entities = Lync.packet(
		Lync.array(
			Lync.struct({ id = Byte, x = Byte, y = Byte, z = Byte, orientation = Byte, animation = Byte }),
			0,
			1000
		)
	),
	Strings = Lync.packet(Lync.array(Lync.str(0, 50), 0, 500)),
	Numbers = Lync.packet(Lync.array(Byte, 0, 2000)),
	Dict = Lync.packet(Lync.map(Lync.str(0, 50), Byte, 0, 500)),
	Vectors = Lync.packet(Lync.array(Lync.vec3(), 0, 500)),
})

local GameNet = Lync.define("game", {
	State = Lync.packet(
		Lync.array(Lync.struct({ id = Short, position = Lync.vec3(), yaw = Short, health = Byte }), 0, 64)
	),
	Hit = Lync.packet(Lync.struct({ attacker = Short, target = Short, damage = Byte })),
	Pickup = Lync.packet(Lync.struct({ item = Short, slot = Byte })),
	Chat = Lync.packet(Lync.str(0, 200)),
	Input = Lync.packet(Lync.struct({
		move = Lync.vec3(),
		look = Lync.vec3(),
		jump = Lync.bool(),
		fire = Lync.bool(),
		crouch = Lync.bool(),
	})),
})

local Benches = require("../Benches")

Lync.start()
RunService.PostSimulation:Connect(function()
	Lync.flush()
end)

local function Sender(Name: string): (Player, any) -> ()
	local Packet = GameNet[Name]
	return function(Player: Player, Data: any)
		Packet:fireClient(Player, Data)
	end
end

return {
	Fire = (function()
		local Fire: { [string]: (any) -> () } = {}
		for Name in Benches do
			Fire[Name] = function(Data: any)
				Net[Name]:fireServer(Data)
			end
		end
		return Fire
	end)(),
	On = function(Name: string, Callback: (Player, any) -> ())
		Net[Name]:onServer(function(Data: any, Player: Player)
			Callback(Player, Data)
		end)
	end,
	Game = {
		Send = { State = Sender("State"), Hit = Sender("Hit"), Pickup = Sender("Pickup"), Chat = Sender("Chat") },
		On = function(Name: string, Callback: (any) -> ())
			GameNet[Name]:onClient(function(Data: any)
				Callback(Data)
			end)
		end,
		Fire = {
			Input = function(Data: any)
				GameNet.Input:fireServer(Data)
			end,
		},
		Listen = function(Name: string, Callback: (Player, any) -> ())
			GameNet[Name]:onServer(function(Data: any, Player: Player)
				Callback(Player, Data)
			end)
		end,
	},
}
