--!strict

local ReplicatedStorage = game:GetService("ReplicatedStorage")
local RunService = game:GetService("RunService")

local Benches = require("../Benches")
local Game = require("../Scenario") :: any

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
	Events[Name] = Remote(`Roblox{Name}`)
end

local GameEvents: { [string]: RemoteEvent } = {}
local Send: { [string]: (Player, any) -> () } = {}
for _, Name in Game.SERVER do
	local Event = Remote(`RobloxGame{Name}`)
	GameEvents[Name] = Event
	Send[Name] = function(Player: Player, Data: any)
		Event:FireClient(Player, Data)
	end
end
local GameFire: { [string]: (any) -> () } = {}
for _, Name in Game.CLIENT do
	local Event = Remote(`RobloxGame{Name}`)
	GameEvents[Name] = Event
	GameFire[Name] = function(Data: any)
		Event:FireServer(Data)
	end
end

local Fire: { [string]: (any) -> () } = {}
for Name, Event in Events do
	Fire[Name] = function(Data: any)
		Event:FireServer(Data)
	end
end

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
		Fire = GameFire,
		Listen = function(Name: string, Callback: (Player, any) -> ())
			GameEvents[Name].OnServerEvent:Connect(Callback)
		end,
	},
}
