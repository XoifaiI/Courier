--!strict

local RunService = game:GetService("RunService")
local Stats = game:GetService("Stats")

local Benches = require("../Shared/Benches") :: any

local WINDOW = 0.5
local NORMAL_FRAMERATE = 60

type Result = {
	Skipped: boolean?,
	Sent: number,
	Framerate: { number },
	Bandwidth: { number },
	Heap: { number },
}

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

local function Spread(Framework: any, Samples: { number }, Format: string): string
	local Low = string.format(Format, Framework.Percentile(Samples, 10))
	local High = string.format(Format, Framework.Percentile(Samples, 90))
	return `{Low} to {High}`
end

local function Server(Framework: any)
	local Received: { [string]: { [string]: number } } = {}
	local ServerHeap: { [string]: number } = {}
	local Running: string? = nil

	for _, Tool in Framework.Tools() do
		local Mode = Framework.Mode(Tool)
		for Name, Data in Benches do
			Received[Name] = Received[Name] or {}
			local Counts = Received[Name]
			Counts[Tool] = 0
			Mode.On(Name, function(_: Player, Value: any)
				Counts[Tool] += 1
				if Counts[Tool] == 1 and not Same(Data, Value) then
					warn(`Bench: {Tool} delivered wrong data for {Name}`)
				end
			end)
		end
	end

	Framework.Listen("Running", function(_: Player, Key: string?)
		Running = Key
	end)

	RunService.Heartbeat:Connect(function()
		if Running then
			ServerHeap[Running] = math.max(ServerHeap[Running] or 0, gcinfo() / 1024)
		end
	end)

	Framework.Handle("Report", function(_: Player, Results: { [string]: { [string]: Result } })
		local Out: { string } = {}
		local Order: { string } = Framework.Tools()
		for Name, Tools in Results do
			local Ranked = table.clone(Order)
			table.sort(Ranked, function(A: string, B: string): boolean
				return Framework.Median(Tools[A].Framerate) > Framework.Median(Tools[B].Framerate)
			end)
			local Rows = Framework.Table({
				Name,
				"FPS",
				"FPS P10 to P90",
				"Kbps",
				"Lost",
				"Client heap MB",
				"Server heap MB",
				"Samples",
			})
			for _, Tool in Ranked do
				local Result = Tools[Tool]
				if Result.Skipped then
					Rows.Row(Tool, "skipped", "", "", "", "", "", "")
					continue
				end
				local Lost = (1 - Received[Name][Tool] / math.max(Result.Sent, 1)) * 100
				Rows.Row(
					Tool,
					string.format("%.0f", Framework.Median(Result.Framerate)),
					Spread(Framework, Result.Framerate, "%.0f"),
					string.format("%.1f", Framework.Median(Result.Bandwidth)),
					string.format("%.1f%%", Lost),
					string.format("%.1f", Framework.Median(Result.Heap)),
					string.format("%.1f", ServerHeap[`{Name}/{Tool}`] or 0),
					tostring(#Result.Framerate)
				)
			end
			table.insert(Out, (Rows.Text()))
		end
		Framework.Print("Throughput results", table.concat(Out, "\n\n"))
		return true
	end)
end

local function Measure(Framework: any, Mode: any, Name: string, Data: any, Result: Result)
	local Fire = Mode.Fire[Name]
	local Fires = Framework.Config.FiresPerFrame
	local Tick = Framework.Windows(WINDOW)
	local Frames = 0
	local Connection = RunService.PostSimulation:Connect(function(DeltaTime: number)
		Frames += 1
		local Ended, Kept, Window = Tick(DeltaTime)
		if Ended then
			if Kept then
				local Framerate = Frames / Window
				table.insert(Result.Framerate, Framerate)
				table.insert(Result.Bandwidth, Stats.DataSendKbps * (NORMAL_FRAMERATE / Framerate))
				table.insert(Result.Heap, gcinfo() / 1024)
			end
			Frames = 0
		end
		for _ = 1, Fires do
			Fire(Data)
		end
		Result.Sent += Fires
	end)
	task.wait(Framework.Config.Seconds)
	Connection:Disconnect()
end

local function Client(Framework: any)
	local Camera = workspace.CurrentCamera
	Camera.FieldOfView = 1
	Camera.CFrame = CFrame.new(0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0)

	local Results: { [string]: { [string]: Result } } = {}
	for Name in Benches do
		Results[Name] = {}
		for _, Tool in Framework.Tools() do
			local Skip = Framework.Mode(Tool).Skip
			Results[Name][Tool] = {
				Skipped = Skip ~= nil and Skip[Name] == true,
				Sent = 0,
				Framerate = {},
				Bandwidth = {},
				Heap = {},
			}
		end
	end

	for Round = 1, Framework.Config.Rounds do
		for Name, Data in Benches do
			for _, Tool in Framework.Shuffled(Framework.Tools(), Round * 1000 + #Name) do
				local Result = Results[Name][Tool]
				if Result.Skipped then
					continue
				end
				print(`Bench: round {Round}, {Name} with {Tool}`)
				Framework.Notify("Running", `{Name}/{Tool}`)
				Measure(Framework, Framework.Mode(Tool), Name, Data, Result)
				Framework.Notify("Running", nil)
				Framework.Drain()
			end
		end
	end
	Framework.Call("Report", Results)
end

return { Server = Server, Client = Client }
