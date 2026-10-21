--!strict

local ReplicatedStorage = game:GetService("ReplicatedStorage")
local RunService = game:GetService("RunService")

local Squash = require("../../Vendor/squash") :: any
local Benches = require("../Benches")
local Game = require("../Scenario") :: any

local CAPACITY = 4096

local Byte = Squash.u8()
local Short = Squash.u16()
local Text = Squash.string()
local Vector = Squash.Vector3(Squash.f32())
local Boolean = Squash.boolean()

local Serializers: { [string]: any } = {
	Booleans = Squash.array(Boolean),
	Entities = Squash.array(
		Squash.record({ id = Byte, x = Byte, y = Byte, z = Byte, orientation = Byte, animation = Byte })
	),
	Strings = Squash.array(Text),
	Numbers = Squash.array(Byte),
	Dict = Squash.map(Text, Byte),
	Vectors = Squash.array(Vector),
}

local GameSerializers: { [string]: any } = {
	State = Squash.array(Squash.record({ id = Short, position = Vector, yaw = Short, health = Byte })),
	Hit = Squash.record({ attacker = Short, target = Short, damage = Byte }),
	Pickup = Squash.record({ item = Short, slot = Byte }),
	Chat = Text,
	Input = Squash.record({ move = Vector, look = Vector, jump = Boolean, fire = Boolean, crouch = Boolean }),
}

local function Remote(Name: string): RemoteEvent
	if RunService:IsServer() then
		local Event = Instance.new("RemoteEvent")
		Event.Name = Name
		Event.Parent = ReplicatedStorage
		return Event
	end
	return ReplicatedStorage:WaitForChild(Name) :: RemoteEvent
end

local Events: { [string]: RemoteEvent } = {}
for Name in Benches do
	Events[Name] = Remote(`Squash{Name}`)
end

local GetBuf = Squash.getbuf
local SetPos = Squash.setpos
local ToBuffer = Squash.tobuffer
local FromBuffer = Squash.frombuffer

local Cursor = Squash.cursor(CAPACITY)
local Start = Squash.getpos(GetBuf(Cursor))

local Fire: { [string]: (any) -> () } = {}
for Name, Event in Events do
	local Ser = Serializers[Name].ser
	Fire[Name] = function(Data: any)
		SetPos(GetBuf(Cursor), Start)
		Ser(Cursor, Data)
		Event:FireServer(ToBuffer(Cursor))
	end
end

local GameEvents: { [string]: RemoteEvent } = {}
local Send: { [string]: (Player, any) -> () } = {}
for _, Name in Game.SERVER do
	local Event = Remote(`SquashGame{Name}`)
	local Ser = GameSerializers[Name].ser
	GameEvents[Name] = Event
	Send[Name] = function(Player: Player, Data: any)
		SetPos(GetBuf(Cursor), Start)
		Ser(Cursor, Data)
		Event:FireClient(Player, ToBuffer(Cursor))
	end
end
local GameFire: { [string]: (any) -> () } = {}
for _, Name in Game.CLIENT do
	local Event = Remote(`SquashGame{Name}`)
	local Ser = GameSerializers[Name].ser
	GameEvents[Name] = Event
	GameFire[Name] = function(Data: any)
		SetPos(GetBuf(Cursor), Start)
		Ser(Cursor, Data)
		Event:FireServer(ToBuffer(Cursor))
	end
end

return {
	Fire = Fire,
	On = function(Name: string, Callback: (Player, any) -> ())
		local Des = Serializers[Name].des
		Events[Name].OnServerEvent:Connect(function(Player: Player, Buffer: buffer)
			Callback(Player, Des(FromBuffer(Buffer)))
		end)
	end,
	Game = {
		Send = Send,
		On = function(Name: string, Callback: (any) -> ())
			local Des = GameSerializers[Name].des
			GameEvents[Name].OnClientEvent:Connect(function(Buffer: buffer)
				Callback(Des(FromBuffer(Buffer)))
			end)
		end,
		Fire = GameFire,
		Listen = function(Name: string, Callback: (Player, any) -> ())
			local Des = GameSerializers[Name].des
			GameEvents[Name].OnServerEvent:Connect(function(Player: Player, Buffer: buffer)
				Callback(Player, Des(FromBuffer(Buffer)))
			end)
		end,
	},
}
