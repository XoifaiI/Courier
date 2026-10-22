--!strict

local RunService = game:GetService("RunService")

local Config = require("./Config")
local Libraries = require("./Libraries")
local Tests = require("./Tests")

export type Library = Libraries.Library

export type Table = {
	Row: (...string) -> (),
	Text: () -> string,
}

local IS_SERVER = RunService:IsServer()
local ROOT = script.Parent :: Instance

local Calls: RemoteFunction
local Notices: RemoteEvent
if IS_SERVER then
	Calls = Instance.new("RemoteFunction")
	Calls.Name = "Calls"
	Calls.Parent = ROOT
	Notices = Instance.new("RemoteEvent")
	Notices.Name = "Notices"
	Notices.Parent = ROOT
else
	Calls = ROOT:WaitForChild("Calls") :: RemoteFunction
	Notices = ROOT:WaitForChild("Notices") :: RemoteEvent
end

local Handlers: { [string]: (Player, ...any) -> ...any } = {}
local Listeners: { [string]: (Player, ...any) -> () } = {}

local function Size(Value: any, Depth: number): number
	local Kind = typeof(Value)
	if Kind == "buffer" then
		return buffer.len(Value)
	elseif Kind == "string" then
		return #Value
	elseif Kind == "table" and Depth < 16 then
		local Total = 0
		for Key, Field in Value do
			Total += Size(Key, Depth + 1) + Size(Field, Depth + 1)
		end
		return Total
	elseif Kind == "Vector3" then
		return 12
	elseif Kind == "number" then
		return 8
	elseif Kind == "boolean" then
		return 1
	end
	return 4
end

local Framework = {}

Framework.Config = Config

function Framework.Libraries(): { string }
	return Config.Libraries or Libraries.Available()
end

function Framework.Library(Name: string): Library
	return Libraries.Load(Name)
end

function Framework.Tests(): { string }
	if Config.Tests then
		return Config.Tests
	end
	local Names: { string } = {}
	for Name in Tests do
		table.insert(Names, Name)
	end
	table.sort(Names)
	return Names
end

function Framework.Pool(Test: string): { any }
	return Tests[Test]
end

function Framework.Size(...: any): number
	local Total = 0
	for Index = 1, select("#", ...) do
		Total += Size((select(Index, ...)), 0)
	end
	return Total
end

function Framework.Handle(Name: string, Handler: (Player, ...any) -> ...any)
	Handlers[Name] = Handler
end

function Framework.Listen(Name: string, Listener: (Player, ...any) -> ())
	Listeners[Name] = Listener
end

function Framework.Call(Name: string, ...: any): ...any
	return Calls:InvokeServer(Name, ...)
end

function Framework.Notify(Name: string, ...: any)
	Notices:FireServer(Name, ...)
end

function Framework.Drain()
	RunService.Heartbeat:Wait()
	RunService.Heartbeat:Wait()
	Framework.Call("Drain")
end

function Framework.Print(Title: string, Text: string)
	if IS_SERVER then
		print(`{Title}\n{Text}`)
	else
		Framework.Call("Print", Title, Text)
	end
end

function Framework.Windows(Length: number): (DeltaTime: number) -> (boolean, boolean, number)
	local Total, Count = 0, 0
	return function(DeltaTime: number): (boolean, boolean, number)
		Total += DeltaTime
		if Total < Length then
			return false, false, 0
		end
		local Window = Total
		Total = 0
		Count += 1
		return true, Count > 1, Window
	end
end

function Framework.Shuffled<T>(List: { T }, Seed: number): { T }
	local Out = table.clone(List)
	local Rng = Random.new(Seed)
	for Index = #Out, 2, -1 do
		local Other = Rng:NextInteger(1, Index)
		Out[Index], Out[Other] = Out[Other], Out[Index]
	end
	return Out
end

function Framework.Percentile(Samples: { number }, Percent: number): number
	if #Samples == 0 then
		return 0
	end
	local Sorted = table.clone(Samples)
	table.sort(Sorted)
	return Sorted[math.clamp(math.ceil(#Sorted * Percent / 100), 1, #Sorted)]
end

function Framework.Median(Samples: { number }): number
	return Framework.Percentile(Samples, 50)
end

function Framework.Table(Columns: { string }): Table
	local Lines = { `|{table.concat(Columns, "|")}|`, `|{string.rep("---|", #Columns)}` }
	return {
		Row = function(...: string)
			table.insert(Lines, `|{table.concat({ ... }, "|")}|`)
		end,
		Text = function(): string
			return table.concat(Lines, "\n")
		end,
	}
end

if IS_SERVER then
	Handlers.Drain = function(): boolean
		return true
	end
	Handlers.Print = function(_: Player, Title: string, Text: string): boolean
		Framework.Print(Title, Text)
		return true
	end
	Calls.OnServerInvoke = function(Player: Player, Name: string, ...: any): ...any
		local Handler = Handlers[Name]
		if Handler == nil then
			error(`Bench: nothing handles "{Name}"`)
		end
		return Handler(Player, ...)
	end
	Notices.OnServerEvent:Connect(function(Player: Player, Name: string, ...: any)
		local Listener = Listeners[Name]
		if Listener then
			Listener(Player, ...)
		end
	end)
end

return Framework
