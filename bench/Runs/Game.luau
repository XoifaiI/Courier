--!strict

local ReplicatedStorage = game:GetService("ReplicatedStorage")
local RunService = game:GetService("RunService")
local Stats = game:GetService("Stats")

local Scenario = require("../Shared/Scenario") :: any

local SERVER_SEED = 7
local CLIENT_SEED = 11

type Play = {
	Sent: { [string]: number },
	Inputs: number,
	BadInputs: number,
	SendSeconds: number,
	Frames: number,
	HeapPeak: number,
	SendKbps: { number },
}

local function Playable(Framework: any): { string }
	local Out = {}
	for _, Tool in Framework.Tools() do
		if Framework.Mode(Tool).Game then
			table.insert(Out, Tool)
		end
	end
	return Out
end

local function Size(Value: any, Depth: number): number
	local Kind = typeof(Value)
	if Kind == "buffer" then
		return buffer.len(Value)
	elseif Kind == "string" then
		return #Value
	elseif Kind == "table" and Depth < 8 then
		local Total = 0
		for Key, Field in Value do
			Total += Size(Key, Depth + 1) + Size(Field, Depth + 1)
		end
		return Total
	elseif Kind == "Vector3" then
		return 12
	elseif Kind == "boolean" then
		return 1
	end
	return 4
end

local function Server(Framework: any)
	local Current: Play? = nil
	local CurrentTool = ""

	for _, Tool in Playable(Framework) do
		Framework.Mode(Tool).Game.Listen("Input", function(_: Player, Value: any)
			local Play = Current
			if Play == nil or CurrentTool ~= Tool then
				return
			end
			Play.Inputs += 1
			if not Scenario.Check("Input", Value) then
				Play.BadInputs += 1
			end
		end)
	end

	Framework.Handle("Play", function(Player: Player, Tool: string): Play
		local Mode = Framework.Mode(Tool).Game
		local Play: Play =
			{ Sent = {}, Inputs = 0, BadInputs = 0, SendSeconds = 0, Frames = 0, HeapPeak = gcinfo(), SendKbps = {} }
		for _, Name in Scenario.SERVER do
			Play.Sent[Name] = 0
		end
		Current = Play
		CurrentTool = Tool
		local Next = Scenario.Server(SERVER_SEED)
		local Tick = Framework.Windows(1)
		local Elapsed = 0
		while Elapsed < Framework.Config.Seconds do
			local Before = os.clock()
			for _, Message in Next() do
				Mode.Send[Message[1]](Player, Message[2])
				Play.Sent[Message[1]] += 1
			end
			Play.SendSeconds += os.clock() - Before
			Play.Frames += 1
			Play.HeapPeak = math.max(Play.HeapPeak, gcinfo())
			local DeltaTime = RunService.Heartbeat:Wait()
			Elapsed += DeltaTime
			local _, Kept = Tick(DeltaTime)
			if Kept then
				table.insert(Play.SendKbps, Stats.DataSendKbps)
			end
		end
		for _ = 1, 3 do
			RunService.Heartbeat:Wait()
			Play.HeapPeak = math.max(Play.HeapPeak, gcinfo())
		end
		return Play
	end)

	Framework.Handle("Finish", function(): Play?
		local Play = Current
		Current = nil
		CurrentTool = ""
		return Play
	end)
end

local function Client(Framework: any)
	local RawBytes, RawCalls = 0, 0
	local Measuring = false

	local function Listen(Item: Instance)
		if Item:IsA("RemoteEvent") or Item:IsA("UnreliableRemoteEvent") then
			local Event = (Item :: any).OnClientEvent :: RBXScriptSignal
			Event:Connect(function(...)
				if not Measuring then
					return
				end
				RawCalls += 1
				for Index = 1, select("#", ...) do
					RawBytes += Size((select(Index, ...)), 0)
				end
			end)
		end
	end

	for _, Root in { ReplicatedStorage, workspace } :: { Instance } do
		for _, Item in Root:GetDescendants() do
			Listen(Item)
		end
		Root.DescendantAdded:Connect(Listen)
	end

	local Received: { [string]: number } = {}
	local Bad = 0
	local CurrentTool = ""
	local Tools = Playable(Framework)

	for _, Tool in Tools do
		for _, Name in Scenario.SERVER do
			Framework.Mode(Tool).Game.On(Name, function(Value: any)
				if CurrentTool ~= Tool then
					return
				end
				Received[Name] = (Received[Name] or 0) + 1
				if not Scenario.Check(Name, Value) then
					Bad += 1
				end
			end)
		end
	end

	local Results = Framework.Table({
		"Tool",
		"Bytes to client per frame",
		"Remote calls to client per frame",
		"Server to client Kbps",
		"Client to server Kbps",
		"Lost server messages",
		"Lost inputs",
		"Bad values",
		"Server send time (us/frame)",
		"Client heap (MB)",
		"Server heap peak (MB)",
	})

	for _, Tool in Tools do
		print(`Bench: game with {Tool}`)
		local Mode = Framework.Mode(Tool).Game
		CurrentTool = Tool
		Received = {}
		Bad = 0
		local Send, Heap = {}, {}
		local Inputs = 0
		local Tick = Framework.Windows(1)
		local Next = Scenario.Client(CLIENT_SEED)
		local Connection = RunService.PostSimulation:Connect(function(DeltaTime: number)
			for _, Message in Next() do
				Mode.Fire[Message[1]](Message[2])
				Inputs += 1
			end
			local _, Kept = Tick(DeltaTime)
			if Kept then
				table.insert(Send, Stats.DataSendKbps)
				table.insert(Heap, gcinfo() / 1024)
			end
		end)
		RawBytes, RawCalls = 0, 0
		Measuring = true
		local Played = Framework.Call("Play", Tool)
		Measuring = false
		Connection:Disconnect()
		Framework.Drain()
		local Finished = Framework.Call("Finish")
		CurrentTool = ""

		local Sent, Got = 0, 0
		for Name, Count in Played.Sent do
			Sent += Count
			Got += math.min(Received[Name] or 0, Count)
		end
		local Frames = math.max(Played.Frames, 1)
		Results.Row(
			Tool,
			string.format("%.0f", RawBytes / Frames),
			string.format("%.2f", RawCalls / Frames),
			string.format("%.2f", Framework.Median(Played.SendKbps)),
			string.format("%.2f", Framework.Median(Send)),
			`{Sent - Got} of {Sent}`,
			`{Inputs - Finished.Inputs} of {Inputs}`,
			tostring(Bad + Finished.BadInputs),
			string.format("%.0f", Played.SendSeconds / Frames * 1e6),
			string.format("%.1f", Framework.Median(Heap)),
			string.format("%.1f", Played.HeapPeak / 1024)
		)
	end

	Framework.Print("Game results", Results.Text())
end

return { Server = Server, Client = Client }
