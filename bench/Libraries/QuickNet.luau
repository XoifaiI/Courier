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

local Fire: { [string]: (any) -> () } = {}
for Test, Target in Events do
	Fire[Test] = function(Value: any)
		Target:FireServer(Value)
	end
end

return {
	Tests = TESTS,
	Fire = Fire,
	On = function(Test: string, Handler: (Player, any) -> ())
		Events[Test].OnServerEvent:Connect(Handler)
	end,
}
