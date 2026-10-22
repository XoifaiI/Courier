--!strict

local ReplicatedStorage = game:GetService("ReplicatedStorage")
local RunService = game:GetService("RunService")
local Stats = game:GetService("Stats")

local Framework = require("../Framework")
local Scenario = require("../Scenario")

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

local function Server()
	local Current: Play? = nil
	local CurrentLibrary = ""

	for _, Name in Framework.Libraries() do
		Framework.Library(Name).Game.OnInput(function(_: Player, Value: any)
			local Play = Current
			if Play == nil or CurrentLibrary ~= Name then
				return
			end
			Play.Inputs += 1
			if not Scenario.Check("Input", Value) then
				Play.BadInputs += 1
			end
		end)
	end

	Framework.Handle("Play", function(Player: Player, Name: string): Play
		local Send = Framework.Library(Name).Game.Send
		local Play: Play = {
			Sent = {},
			Inputs = 0,
			BadInputs = 0,
			SendSeconds = 0,
			Frames = 0,
			HeapPeak = gcinfo(),
			SendKbps = {},
		}
		for _, Message in Scenario.SERVER do
			Play.Sent[Message] = 0
		end
		Current = Play
		CurrentLibrary = Name
		local Next = Scenario.Server(SERVER_SEED)
		local Tick = Framework.Windows(1)
		local Elapsed = 0
		while Elapsed < Framework.Config.Seconds * 4 do
			local Before = os.clock()
			for _, Message in Next() do
				Send[Message[1]](Player, Message[2])
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
		CurrentLibrary = ""
		return Play
	end)
end

local function Client()
	local RawBytes, RawCalls = 0, 0
	local Measuring = false

	local function Watch(Item: Instance)
		if not (Item:IsA("RemoteEvent") or Item:IsA("UnreliableRemoteEvent")) then
			return
		end
		((Item :: any).OnClientEvent :: RBXScriptSignal):Connect(function(...: any)
			if Measuring then
				RawCalls += 1
				RawBytes += Framework.Size(...)
			end
		end)
	end
	for _, Root in { ReplicatedStorage, workspace } :: { Instance } do
		for _, Item in Root:GetDescendants() do
			Watch(Item)
		end
		Root.DescendantAdded:Connect(Watch)
	end

	local Received: { [string]: number } = {}
	local Bad = 0
	local CurrentLibrary = ""

	for _, Name in Framework.Libraries() do
		local Game = Framework.Library(Name).Game
		for _, Message in Scenario.SERVER do
			Game.On(Message, function(Value: any)
				if CurrentLibrary ~= Name then
					return
				end
				Received[Message] = (Received[Message] or 0) + 1
				if not Scenario.Check(Message, Value) then
					Bad += 1
				end
			end)
		end
	end

	local Results = Framework.Table({
		"Library",
		"Bytes per frame",
		"Calls per frame",
		"Server KB/s",
		"Client KB/s",
		"Lost messages",
		"Lost inputs",
		"Bad values",
		"Server send µs per frame",
		"Client MB",
		"Server MB",
	})

	for _, Name in Framework.Libraries() do
		print(`Bench: game with {Name}`)
		local Game = Framework.Library(Name).Game
		CurrentLibrary = Name
		Received = {}
		Bad = 0
		local Send, Heap = {}, {}
		local Inputs = 0
		local Tick = Framework.Windows(1)
		local Next = Scenario.Client(CLIENT_SEED)
		local Connection = RunService.PostSimulation:Connect(function(DeltaTime: number)
			for _, Message in Next() do
				Game.Input(Message[2])
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
		local Played = Framework.Call("Play", Name)
		Measuring = false
		Connection:Disconnect()
		Framework.Drain()
		local Finished = Framework.Call("Finish")
		CurrentLibrary = ""

		local Sent, Got = 0, 0
		for Message, Count in Played.Sent do
			Sent += Count
			Got += math.min(Received[Message] or 0, Count)
		end
		local Frames = math.max(Played.Frames, 1)
		Results.Row(
			Name,
			string.format("%.0f", RawBytes / Frames),
			string.format("%.2f", RawCalls / Frames),
			string.format("%.1f", Framework.Median(Played.SendKbps)),
			string.format("%.1f", Framework.Median(Send)),
			`{Sent - Got} of {Sent}`,
			`{Inputs - Finished.Inputs} of {Inputs}`,
			tostring(Bad + Finished.BadInputs),
			string.format("%.0f", Played.SendSeconds / Frames * 1e6),
			string.format("%.0f", Framework.Median(Heap)),
			string.format("%.0f", Played.HeapPeak / 1024)
		)
	end

	Framework.Print("Game results", Results.Text())
end

return { Server = Server, Client = Client }
