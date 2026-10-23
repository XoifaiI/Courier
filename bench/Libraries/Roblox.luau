--!strict

local ReplicatedStorage = game:GetService("ReplicatedStorage")
local RunService = game:GetService("RunService")

local TESTS = { "Booleans", "Entities", "Strings", "Numbers", "Dict", "Vectors" }

local function Remote(Name: string): RemoteEvent
	if RunService:IsServer() then
		local Event = Instance.new("RemoteEvent")
		Event.Name = `Roblox{Name}`
		Event.Parent = ReplicatedStorage
		return Event
	end
	return ReplicatedStorage:WaitForChild(`Roblox{Name}`) :: RemoteEvent
end

local Remotes: { [string]: RemoteEvent } = {}
for _, Name in TESTS do
	Remotes[Name] = Remote(Name)
end

local Fire: { [string]: (any) -> () } = {}
for _, Test in TESTS do
	local Target = Remotes[Test]
	Fire[Test] = function(Value: any)
		Target:FireServer(Value)
	end
end

return {
	Tests = TESTS,
	Fire = Fire,
	On = function(Test: string, Handler: (Player, any) -> ())
		Remotes[Test].OnServerEvent:Connect(Handler)
	end,
}
