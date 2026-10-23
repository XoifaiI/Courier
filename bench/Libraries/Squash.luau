--!strict

local ReplicatedStorage = game:GetService("ReplicatedStorage")
local RunService = game:GetService("RunService")

local Squash = require("../Vendor/squash") :: any

local TESTS = { "Booleans", "Entities", "Strings", "Numbers", "Dict", "Vectors" }

local CAPACITY = 4096

local Byte = Squash.u8()
local Text = Squash.string()
local Vector = Squash.Vector3(Squash.f32())
local Flag = Squash.boolean()

local SERIALIZERS: { [string]: any } = {
	Booleans = Squash.array(Flag),
	Entities = Squash.array(
		Squash.record({ id = Byte, x = Byte, y = Byte, z = Byte, orientation = Byte, animation = Byte })
	),
	Strings = Squash.array(Text),
	Numbers = Squash.array(Byte),
	Dict = Squash.map(Text, Byte),
	Vectors = Squash.array(Vector),
}

local Cursor = Squash.cursor(CAPACITY)
local Start = Squash.getpos(Squash.getbuf(Cursor))

local function Remote(Name: string): RemoteEvent
	if RunService:IsServer() then
		local Event = Instance.new("RemoteEvent")
		Event.Name = `Squash{Name}`
		Event.Parent = ReplicatedStorage
		return Event
	end
	return ReplicatedStorage:WaitForChild(`Squash{Name}`) :: RemoteEvent
end

local function Write(Name: string, Value: any): buffer
	Squash.setpos(Squash.getbuf(Cursor), Start)
	SERIALIZERS[Name].ser(Cursor, Value)
	return Squash.tobuffer(Cursor)
end

local function Read(Name: string, Data: buffer): any
	return SERIALIZERS[Name].des(Squash.frombuffer(Data))
end

local Remotes: { [string]: RemoteEvent } = {}
for Name in SERIALIZERS do
	Remotes[Name] = Remote(Name)
end

local Fire: { [string]: (any) -> () } = {}
for _, Test in TESTS do
	local Target = Remotes[Test]
	Fire[Test] = function(Value: any)
		Target:FireServer(Write(Test, Value))
	end
end

return {
	Tests = TESTS,
	Fire = Fire,
	On = function(Test: string, Handler: (Player, any) -> ())
		Remotes[Test].OnServerEvent:Connect(function(Player: Player, Data: unknown)
			if typeof(Data) == "buffer" then
				Handler(Player, Read(Test, Data))
			end
		end)
	end,
}
