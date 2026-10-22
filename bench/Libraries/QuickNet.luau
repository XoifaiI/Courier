--!strict

local QuickNet = require("../Vendor/quicknet") :: any

local TESTS = { "Booleans", "Dict", "Entities", "Numbers", "Strings", "Vectors" }

local Data = QuickNet.Data

local LIMIT = 1000000000

local function Event(Name: string, Schema: any): any
	return QuickNet:register(Name, Schema):SetRateLimit(LIMIT, 30)
end

local Events: { [string]: any } = {
	Booleans = Event("Booleans", { Data.Boolean }),
	Entities = Event("Entities", {
		{
			id = Data.NumberU8,
			x = Data.NumberU8,
			y = Data.NumberU8,
			z = Data.NumberU8,
			orientation = Data.NumberU8,
			animation = Data.NumberU8,
		},
	}),
	Strings = Event("Strings", { Data.String }),
	Numbers = Event("Numbers", { Data.NumberU8 }),
	Dict = Event("Dict", { [Data.String] = Data.NumberU8 }),
	Vectors = Event("Vectors", { Data.Vector3F32 }),
}

local GameEvents: { [string]: any } = {
	State = Event("State", {
		{ id = Data.NumberU16, position = Data.Vector3F32, yaw = Data.NumberU16, health = Data.NumberU8 },
	}),
	Hit = Event("Hit", { attacker = Data.NumberU16, target = Data.NumberU16, damage = Data.NumberU8 }),
	Pickup = Event("Pickup", { item = Data.NumberU16, slot = Data.NumberU8 }),
	Chat = Event("Chat", Data.String),
	Input = Event("Input", {
		move = Data.Vector3F32,
		look = Data.Vector3F32,
		jump = Data.Boolean,
		fire = Data.Boolean,
		crouch = Data.Boolean,
	}),
}

local Fire: { [string]: (any) -> () } = {}
for Test, Target in Events do
	Fire[Test] = function(Value: any)
		Target:FireServer(Value)
	end
end

local Send: { [string]: (Player, any) -> () } = {}
for _, Name in { "State", "Hit", "Pickup", "Chat" } do
	local Target = GameEvents[Name]
	Send[Name] = function(Player: Player, Value: any)
		Target:FireClient(Player, Value)
	end
end

return {
	Tests = TESTS,
	Fire = Fire,
	On = function(Test: string, Handler: (Player, any) -> ())
		Events[Test].OnServerEvent:Connect(Handler)
	end,
	Game = {
		Send = Send,
		On = function(Name: string, Handler: (any) -> ())
			GameEvents[Name].OnClientEvent:Connect(Handler)
		end,
		Input = function(Value: any)
			GameEvents.Input:FireServer(Value)
		end,
		OnInput = function(Handler: (Player, any) -> ())
			GameEvents.Input.OnServerEvent:Connect(Handler)
		end,
	},
}
