--!strict

local PakNet = require("../../Vendor/paknet") :: any

local Byte = PakNet.UInt8
local Short = PakNet.UInt16
local Vector = PakNet.Vector3()

local function Remote(Datatype: any): any
	return PakNet:DefineRemote({ params = PakNet:Schema(Datatype), remoteType = "r" })
end

local Net = PakNet:Mount(script, {
	Booleans = Remote(PakNet.Array(PakNet.Boolean)),
	Entities = Remote(
		PakNet.Array(
			PakNet.Dictionary({ id = Byte, x = Byte, y = Byte, z = Byte, orientation = Byte, animation = Byte })
		)
	),
	Strings = Remote(PakNet.Array(PakNet.String8)),
	Numbers = Remote(PakNet.Array(Byte)),
	Dict = Remote(PakNet.Map(PakNet.String8, Byte)),
	Vectors = Remote(PakNet.Array(PakNet.Vector3())),
	State = Remote(
		PakNet.Array(PakNet.Dictionary({ id = Short, position = Vector, yaw = Short, health = Byte }), Byte)
	),
	Hit = Remote(PakNet.Dictionary({ attacker = Short, target = Short, damage = Byte })),
	Pickup = Remote(PakNet.Dictionary({ item = Short, slot = Byte })),
	Chat = Remote(PakNet.String8),
	Input = Remote(PakNet.Dictionary({
		move = Vector,
		look = Vector,
		jump = PakNet.Boolean,
		fire = PakNet.Boolean,
		crouch = PakNet.Boolean,
	})),
})

local Benches = require("../Benches")
local Game = require("../Scenario") :: any

local Send: { [string]: (Player, any) -> () } = {}
for _, Name in Game.SERVER do
	local Event = Net[Name]
	Send[Name] = function(Player: Player, Data: any)
		Event:Fire(Player, Data)
	end
end

local GameFire: { [string]: (any) -> () } = {}
for _, Name in Game.CLIENT do
	local Event = Net[Name]
	GameFire[Name] = function(Data: any)
		Event:FireServer(Data)
	end
end

return {
	Fire = (function()
		local Fire: { [string]: (any) -> () } = {}
		for Name in Benches do
			local Event = Net[Name]
			Fire[Name] = function(Data: any)
				Event:FireServer(Data)
			end
		end
		return Fire
	end)(),
	On = function(Name: string, Callback: (Player, any) -> ())
		Net[Name].OnServerEvent:Connect(Callback)
	end,
	Game = {
		Send = Send,
		On = function(Name: string, Callback: (any) -> ())
			Net[Name].OnClientEvent:Connect(Callback)
		end,
		Fire = GameFire,
		Listen = function(Name: string, Callback: (Player, any) -> ())
			Net[Name].OnServerEvent:Connect(Callback)
		end,
	},
}
