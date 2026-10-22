--!strict

local ReplicatedStorage = game:GetService("ReplicatedStorage")
local RunService = game:GetService("RunService")
local Stats = game:GetService("Stats")

local Framework = require("../Framework")

local WINDOW = 0.25
local NORMAL_FRAMERATE = 60

type Result = {
	Sent: number,
	Framerate: { number },
	Bandwidth: { number },
	Allocated: { number },
}

type Results = { [string]: { [string]: Result } }

local function Same(A: any, B: any): boolean
	if type(A) ~= "table" or type(B) ~= "table" then
		return A == B
	end
	for Key, Value in A do
		if not Same(Value, B[Key]) then
			return false
		end
	end
	for Key in B do
		if A[Key] == nil then
			return false
		end
	end
	return true
end

local function Format(Samples: { number }, Spread: boolean): string
	if Spread then
		return `{string.format("%.0f", Framework.Percentile(Samples, 10))} to {string.format(
			"%.0f",
			Framework.Percentile(Samples, 90)
		)}`
	end
	return string.format("%.0f", Framework.Median(Samples))
end

local function Measure(Fire: (any) -> (), Pool: { any }, Result: Result)
	local Fires = Framework.Config.FiresPerFrame
	local Count = #Pool
	local Next = 0
	local Tick = Framework.Windows(WINDOW)
	local Frames = 0
	local Connection = RunService.PostSimulation:Connect(function(DeltaTime: number)
		Frames += 1
		local Ended, Kept, Window = Tick(DeltaTime)
		if Ended then
			if Kept then
				local Framerate = Frames / Window
				table.insert(Result.Framerate, Framerate)
				table.insert(Result.Bandwidth, Stats.DataSendKbps * NORMAL_FRAMERATE / Framerate)
			end
			Frames = 0
		end
		local Before = gcinfo()
		for _ = 1, Fires do
			Next = Next % Count + 1
			Fire(Pool[Next])
		end
		local After = gcinfo()
		if After >= Before then
			table.insert(Result.Allocated, (After - Before) * 1024 / Fires)
		end
		Result.Sent += Fires
	end)
	task.wait(Framework.Config.Seconds)
	Connection:Disconnect()
end

local function Server()
	local Bench = ReplicatedStorage:WaitForChild("Bench")
	local Received: { [string]: number } = {}
	local Counted: { [string]: number } = {}
	local Bytes: { [string]: number } = {}
	local Rise: { [string]: number } = {}
	local CellStart, CellPeak = 0, 0
	local Wrong: { [string]: boolean } = {}
	local Running: string? = nil
	local First = false

	for _, Name in Framework.Libraries() do
		local Library = Framework.Library(Name)
		for _, Test in Framework.Tests() do
			if not table.find(Library.Tests, Test) then
				continue
			end
			local Key = `{Test}/{Name}`
			local Expected = Framework.Pool(Test)[1]
			Received[Key] = 0
			Counted[Key] = 0
			Bytes[Key] = 0
			Library.On(Test, function(_: Player, Value: any)
				Received[Key] += 1
				if Running ~= Key then
					return
				end
				Counted[Key] += 1
				if First then
					First = false
					if not Same(Expected, Value) then
						Wrong[Key] = true
					end
				end
			end)
		end
	end

	local function Watch(Item: Instance)
		if Item.Parent == Bench or not (Item:IsA("RemoteEvent") or Item:IsA("UnreliableRemoteEvent")) then
			return
		end
		((Item :: any).OnServerEvent :: RBXScriptSignal):Connect(function(_: Player, ...: any)
			local Key = Running
			if Key and Bytes[Key] then
				Bytes[Key] += Framework.Size(...)
			end
		end)
	end
	for _, Root in { ReplicatedStorage, workspace } :: { Instance } do
		for _, Item in Root:GetDescendants() do
			Watch(Item)
		end
		Root.DescendantAdded:Connect(Watch)
	end

	Framework.Listen("Running", function(_: Player, Key: string?)
		local Previous = Running
		if Previous then
			Rise[Previous] = math.max(Rise[Previous] or 0, CellPeak - CellStart)
		end
		Running = Key
		First = Key ~= nil
		CellStart = gcinfo()
		CellPeak = CellStart
	end)

	RunService.Heartbeat:Connect(function()
		if Running then
			CellPeak = math.max(CellPeak, gcinfo())
		end
	end)

	Framework.Handle("Report", function(_: Player, Results: Results, ClientMemory: { [string]: number }): boolean
		local Out: { string } = {}
		local ServerMemory = Framework.Memory()
		local Held: { string } = Framework.Libraries()
		table.sort(Held, function(A: string, B: string): boolean
			return ServerMemory[A] + ClientMemory[A] < ServerMemory[B] + ClientMemory[B]
		end)
		local Memory = Framework.Table({ "Memory held after the run", "Server KB", "Client KB" })
		for _, Name in Held do
			Memory.Row(
				Name,
				string.format("%.0f", ServerMemory[Name] / 1024),
				string.format("%.0f", ClientMemory[Name] / 1024)
			)
		end
		for _, Test in Framework.Tests() do
			local Rows = Framework.Table({
				Test,
				"FPS",
				"FPS P10 to P90",
				"Bytes per message",
				"KB/s at 60 FPS",
				"Lost",
				"Client bytes allocated per message",
				"Server heap rise MB",
			})
			local Ranked: { string } = {}
			for Name in Results[Test] do
				table.insert(Ranked, Name)
			end
			table.sort(Ranked, function(A: string, B: string): boolean
				return Framework.Median(Results[Test][A].Framerate) > Framework.Median(Results[Test][B].Framerate)
			end)
			for _, Name in Ranked do
				local Result = Results[Test][Name]
				local Key = `{Test}/{Name}`
				Rows.Row(
					if Wrong[Key] then `{Name} (wrong data)` else Name,
					Format(Result.Framerate, false),
					Format(Result.Framerate, true),
					string.format("%.0f", Bytes[Key] / math.max(Counted[Key], 1)),
					string.format("%.0f", Framework.Median(Result.Bandwidth)),
					string.format("%.1f%%", (1 - Received[Key] / math.max(Result.Sent, 1)) * 100),
					string.format("%.0f", Framework.Median(Result.Allocated)),
					string.format("%.1f", (Rise[Key] or 0) / 1024)
				)
			end
			for _, Name in Framework.Libraries() do
				if Results[Test][Name] == nil then
					Rows.Row(Name, "unsupported", "", "", "", "", "", "")
				end
			end
			table.insert(Out, Rows.Text())
		end
		table.insert(Out, Memory.Text())
		Framework.Print("Throughput results", table.concat(Out, "\n\n"))
		return true
	end)
end

local function Client()
	local Libraries: { [string]: Framework.Library } = {}
	for _, Name in Framework.Libraries() do
		Libraries[Name] = Framework.Library(Name)
	end

	local Results: Results = {}
	for _, Test in Framework.Tests() do
		Results[Test] = {}
		for Name, Library in Libraries do
			if table.find(Library.Tests, Test) then
				Results[Test][Name] = { Sent = 0, Framerate = {}, Bandwidth = {}, Allocated = {} }
			end
		end
	end

	for Round = 1, Framework.Config.Rounds do
		for _, Test in Framework.Tests() do
			for _, Name in Framework.Shuffled(Framework.Libraries(), Round * 1000 + #Test) do
				local Result = Results[Test][Name]
				if Result == nil then
					continue
				end
				print(`Bench: round {Round}, {Test} with {Name}`)
				Framework.Notify("Running", `{Test}/{Name}`)
				Measure(Libraries[Name].Fire[Test], Framework.Pool(Test), Result)
				Framework.Drain()
				Framework.Notify("Running", nil)
			end
		end
	end
	Framework.Call("Report", Results, Framework.Memory())
end

return { Server = Server, Client = Client }
