--!strict

local LogService = game:GetService("LogService")
local ReplicatedStorage = game:GetService("ReplicatedStorage")
local RunService = game:GetService("RunService")

local Benches = require("../Shared/Benches") :: any
local Validate = require("../Shared/Validate") :: any
local Payloads = require("../Shared/Payloads")

type Payload = Payloads.Payload

local SAMPLES_PER_REMOTE = 6
local FIRES_PER_FRAME = 25

type Category = { Base: number, Peak: number, Sent: number }

type Report = {
	Categories: { [string]: Category },
	Errors: number,
	Warnings: number,
	FirstError: string?,
	Delivered: number,
	Invalid: number,
	FirstInvalid: string?,
	Live: number,
	Expected: number,
	LongestFrame: number,
	HeapBefore: number,
	HeapPeak: number,
	HeapAfter: number,
}

local function Remotes(): { [Instance]: boolean }
	local Found = {}
	for _, Root in { ReplicatedStorage, workspace } :: { Instance } do
		for _, Item in Root:GetDescendants() do
			if Item:IsA("RemoteEvent") or Item:IsA("UnreliableRemoteEvent") or Item:IsA("RemoteFunction") then
				Found[Item] = true
			end
		end
	end
	return Found
end

local function Describe(Value: any, Depth: number?): string
	local Level = Depth or 0
	if type(Value) ~= "table" then
		local Text = if type(Value) == "string" then string.format("%q", string.sub(Value, 1, 20)) else tostring(Value)
		return `{typeof(Value)} {Text}`
	end
	if Level >= 2 then
		return "table"
	end
	local Parts = {}
	for Key, Field in Value do
		table.insert(Parts, `{tostring(Key)}={Describe(Field, Level + 1)}`)
		if #Parts >= 3 then
			table.insert(Parts, "...")
			break
		end
	end
	return `\{{table.concat(Parts, ", ")}\}`
end

local function Worst(Report: Report): string
	local Name, Growth, Sent = "none", 0, 0
	for Kind, Category in Report.Categories do
		if Category.Peak - Category.Base > Growth then
			Name, Growth, Sent = Kind, Category.Peak - Category.Base, Category.Sent
		end
	end
	if Growth == 0 then
		return "none"
	end
	local Ratio = Growth * 1024 / math.max(Sent, 1)
	return string.format("%s: +%.1f MB from %.1f KB sent (%.0fx)", Name, Growth / 1024, Sent / 1024, Ratio)
end

local function Server(Framework: any)
	local Owned: { [string]: { Instance } } = {}
	local Samples: { [Instance]: { Payload } } = {}
	local Reports: { [string]: Report } = {}
	local Current: { Phase: string, Tool: string, Category: Category? } = { Phase = "", Tool = "", Category = nil }

	local Baseline = Remotes()
	for _, Tool in Framework.Tools() do
		local Before = Remotes()
		local Mode = Framework.Mode(Tool)
		local Expected = 0
		for Name in Benches do
			if not (Mode.Skip and Mode.Skip[Name]) then
				Expected += 1
			end
			Mode.On(Name, function(_: Player, Value: any)
				if Current.Tool ~= Tool then
					return
				end
				local Report = Reports[Tool]
				if Current.Phase == "Attack" then
					Report.Delivered += 1
					if not Validate(Name, Value) then
						Report.Invalid += 1
						if Report.FirstInvalid == nil then
							Report.FirstInvalid = `{Name}: {Describe(Value)}`
						end
					end
				elseif Current.Phase == "Live" then
					Report.Live += 1
				end
			end)
		end
		local Mine = {}
		for Item in Remotes() do
			if not Before[Item] then
				table.insert(Mine, Item)
			end
		end
		Owned[Tool] = Mine
		Reports[Tool] = {
			Categories = {},
			Errors = 0,
			Warnings = 0,
			Delivered = 0,
			Invalid = 0,
			Live = 0,
			Expected = Expected,
			LongestFrame = 0,
			HeapBefore = 0,
			HeapPeak = 0,
			HeapAfter = 0,
		}
	end
	if Owned.Courier then
		local Runtime = ReplicatedStorage:WaitForChild("Courier")
		for Item in Baseline do
			if Item:IsDescendantOf(Runtime) then
				table.insert(Owned.Courier, Item)
			end
		end
	end

	for Tool, Items in Owned do
		for _, Item in Items do
			if Item:IsA("RemoteFunction") then
				continue
			end
			local List: { Payload } = {}
			Samples[Item] = List
			local Event = (Item :: any).OnServerEvent :: RBXScriptSignal
			Event:Connect(function(_: Player, ...: any)
				if Current.Phase == "Prime" and Current.Tool == Tool and #List < SAMPLES_PER_REMOTE then
					table.insert(List, table.pack(...) :: any)
				end
			end)
		end
	end

	LogService.MessageOut:Connect(function(Message: string, Kind: Enum.MessageType)
		local Report = Reports[Current.Tool]
		if Report == nil or (Current.Phase ~= "Attack" and Current.Phase ~= "Live") then
			return
		end
		if Kind == Enum.MessageType.MessageError then
			Report.Errors += 1
			if Report.FirstError == nil then
				Report.FirstError = string.sub(Message, 1, 120)
			end
		elseif Kind == Enum.MessageType.MessageWarning then
			Report.Warnings += 1
		end
	end)

	RunService.Heartbeat:Connect(function(DeltaTime: number)
		local Report = Reports[Current.Tool]
		if Report == nil or Current.Phase ~= "Attack" then
			return
		end
		local Heap = gcinfo()
		Report.LongestFrame = math.max(Report.LongestFrame, DeltaTime)
		Report.HeapPeak = math.max(Report.HeapPeak, Heap)
		local Category = Current.Category
		if Category then
			Category.Peak = math.max(Category.Peak, Heap)
		end
	end)

	Framework.Handle("Phase", function(_: Player, Phase: string, Tool: string)
		Current.Category = nil
		local Report = Reports[Tool]
		if Phase == "Attack" then
			Report.HeapBefore = gcinfo()
			Report.HeapPeak = Report.HeapBefore
		elseif Phase == "Done" then
			Report.HeapAfter = gcinfo()
		end
		Current.Phase = Phase
		Current.Tool = Tool
		return true
	end)

	Framework.Handle("Category", function(_: Player, Name: string)
		local Heap = gcinfo()
		local Category = { Base = Heap, Peak = Heap, Sent = 0 }
		Reports[Current.Tool].Categories[Name] = Category
		Current.Category = Category
		return true
	end)

	Framework.Handle("Sent", function(_: Player, Bytes: number)
		local Category = Current.Category
		if Category then
			Category.Sent += Bytes
		end
		return true
	end)

	Framework.Handle("Samples", function(_: Player, Tool: string)
		local Targets = {}
		for _, Item in Owned[Tool] do
			local Kept = {}
			for _, Sample in Samples[Item] or {} do
				table.insert(Kept, { Count = Sample.n, Args = table.move(Sample, 1, Sample.n, 1, {}) })
			end
			table.insert(Targets, { Remote = Item, Samples = Kept })
		end
		return Targets
	end)

	Framework.Handle("Report", function()
		Current.Phase = ""
		local Results = Framework.Table({
			"Tool",
			"Server errors",
			"Warnings",
			"Bad values handed to the game",
			"Values handed to the game",
			"Works after",
			"Longest frame (ms)",
			"Heap growth (MB)",
			"Heap peak over start (MB)",
			"Worst allocation",
		})
		local Examples: { string } = {}
		for _, Tool in Framework.Tools() do
			local Report = Reports[Tool]
			Results.Row(
				Tool,
				tostring(Report.Errors),
				tostring(Report.Warnings),
				tostring(Report.Invalid),
				tostring(Report.Delivered),
				if Report.Live >= Report.Expected then "yes" else `no ({Report.Live}/{Report.Expected})`,
				string.format("%.0f", Report.LongestFrame * 1000),
				string.format("%.1f", (Report.HeapAfter - Report.HeapBefore) / 1024),
				string.format("%.1f", (Report.HeapPeak - Report.HeapBefore) / 1024),
				Worst(Report)
			)
			if Report.FirstError then
				table.insert(Examples, `- {Tool} error: {Report.FirstError}`)
			end
			if Report.FirstInvalid then
				table.insert(Examples, `- {Tool} handed the game: {Report.FirstInvalid}`)
			end
		end
		Framework.Print("Attack results", `{Results.Text()}\n\n{table.concat(Examples, "\n")}`)
		return true
	end)
end

local function Client(Framework: any)
	local function Frames(Count: number)
		for _ = 1, Count do
			RunService.Heartbeat:Wait()
		end
	end

	local function Fire(Remote: Instance, Payload: Payload)
		local Target = Remote :: any
		if Remote:IsA("RemoteFunction") then
			task.spawn(pcall, Target.InvokeServer, Target, table.unpack(Payload, 1, Payload.n))
		else
			pcall(Target.FireServer, Target, table.unpack(Payload, 1, Payload.n))
		end
	end

	local function SendEach(Mode: any)
		for Name, Data in Benches do
			if not (Mode.Skip and Mode.Skip[Name]) then
				Mode.Fire[Name](Data)
				Frames(3)
			end
		end
		Frames(3)
	end

	for _, Tool in Framework.Tools() do
		print(`Bench: attacking {Tool}`)
		local Mode = Framework.Mode(Tool)
		Framework.Call("Phase", "Prime", Tool)
		SendEach(Mode)
		local Targets = Framework.Call("Samples", Tool) :: { any }
		for _, Target in Targets do
			local Rebuilt: { Payload } = {}
			for _, Sample in Target.Samples do
				local Payload = Sample.Args :: Payload
				Payload.n = Sample.Count
				table.insert(Rebuilt, Payload)
			end
			Target.Samples = Rebuilt
		end
		Framework.Call("Phase", "Attack", Tool)
		for _, Category in Payloads.CATEGORIES do
			Framework.Call("Category", Category)
			local Sent = 0
			for _, Target in Targets do
				for Count, Payload in Payloads.Build[Category](Target.Samples) do
					Fire(Target.Remote, Payload)
					Sent += Payloads.Size(Payload)
					if Count % FIRES_PER_FRAME == 0 then
						RunService.Heartbeat:Wait()
					end
				end
			end
			Frames(3)
			Framework.Call("Sent", Sent)
			task.wait(0.25)
		end
		Framework.Call("Phase", "Live", Tool)
		SendEach(Mode)
		task.wait(0.3)
		Framework.Call("Phase", "Done", Tool)
	end
	Framework.Call("Report")
end

return { Server = Server, Client = Client }
