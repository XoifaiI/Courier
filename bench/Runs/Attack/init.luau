--!strict

local LogService = game:GetService("LogService")
local ReplicatedStorage = game:GetService("ReplicatedStorage")
local RunService = game:GetService("RunService")

local Framework = require("../Framework")
local Payloads = require("@self/Payloads")
local Validate = require("@self/Validate")

type Payload = Payloads.Payload

local SAMPLES_PER_REMOTE = 6
local FIRES_PER_FRAME = 25

type Category = { Base: number, Peak: number, Sent: number, Errors: number, Invalid: number }

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
	HeldBefore: number,
	HeldPeak: number,
	HeldAfter: number,
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

local function Server()
	local Owned: { [string]: { Instance } } = {}
	local Samples: { [Instance]: { Payload } } = {}
	local Reports: { [string]: Report } = {}
	local Current: { Phase: string, Tool: string, Category: Category? } = { Phase = "", Tool = "", Category = nil }

	local Baseline = Remotes()
	for _, Tool in Framework.Libraries() do
		local Before = Remotes()
		local Library = Framework.Library(Tool)
		local Expected = 0
		for _, Name in Library.Tests do
			Expected += 1
			Library.On(Name, function(_: Player, Value: any)
				if Current.Tool ~= Tool then
					return
				end
				local Report = Reports[Tool]
				if Current.Phase == "Attack" then
					Report.Delivered += 1
					if not Validate(Name, Value) then
						Report.Invalid += 1
						if Current.Category then
							Current.Category.Invalid += 1
						end
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
			HeldBefore = 0,
			HeldPeak = 0,
			HeldAfter = 0,
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
			if Current.Category then
				Current.Category.Errors += 1
			end
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
		Report.LongestFrame = math.max(Report.LongestFrame, DeltaTime)
		local Category = Current.Category
		if Category then
			Category.Peak = math.max(Category.Peak, gcinfo())
		end
	end)

	Framework.Handle("Phase", function(_: Player, Phase: string, Tool: string)
		Current.Category = nil
		local Report = Reports[Tool]
		if Phase == "Attack" then
			Report.HeldBefore = Framework.Memory()[Tool]
			Report.HeldPeak = Report.HeldBefore
		elseif Phase == "Done" then
			Report.HeldAfter = Framework.Memory()[Tool]
		end
		Current.Phase = Phase
		Current.Tool = Tool
		return true
	end)

	Framework.Handle("Category", function(_: Player, Name: string)
		local Heap = gcinfo()
		local Category = { Base = Heap, Peak = Heap, Sent = 0, Errors = 0, Invalid = 0 }
		Reports[Current.Tool].Categories[Name] = Category
		Current.Category = Category
		return true
	end)

	Framework.Handle("Sent", function(_: Player, Bytes: number)
		local Category = Current.Category
		if Category then
			Category.Sent += Bytes
		end
		local Report = Reports[Current.Tool]
		Report.HeldPeak = math.max(Report.HeldPeak, Framework.Memory()[Current.Tool])
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
			"Library memory before (KB)",
			"Library memory peak (KB)",
			"Library memory after (KB)",
			"Worst heap spike",
		})
		local Examples: { string } = {}
		for _, Tool in Framework.Libraries() do
			local Report = Reports[Tool]
			Results.Row(
				Tool,
				tostring(Report.Errors),
				tostring(Report.Warnings),
				tostring(Report.Invalid),
				tostring(Report.Delivered),
				if Report.Live >= Report.Expected then "yes" else `no ({Report.Live}/{Report.Expected})`,
				string.format("%.0f", Report.LongestFrame * 1000),
				string.format("%.0f", Report.HeldBefore / 1024),
				string.format("%.0f", Report.HeldPeak / 1024),
				string.format("%.0f", Report.HeldAfter / 1024),
				Worst(Report)
			)
			if Report.FirstError then
				table.insert(Examples, `- {Tool} error: {Report.FirstError}`)
			end
			if Report.FirstInvalid then
				table.insert(Examples, `- {Tool} handed the game: {Report.FirstInvalid}`)
			end
		end
		local Columns = { "Errors / bad values by attack" }
		for _, Name in Payloads.CATEGORIES do
			table.insert(Columns, Name)
		end
		local ByAttack = Framework.Table(Columns)
		for _, Tool in Framework.Libraries() do
			local Cells = { Tool }
			for _, Name in Payloads.CATEGORIES do
				local Category = Reports[Tool].Categories[Name]
				local Clean = Category == nil or (Category.Errors == 0 and Category.Invalid == 0)
				table.insert(Cells, if Clean then "-" else `{Category.Errors} / {Category.Invalid}`)
			end
			ByAttack.Row(table.unpack(Cells))
		end
		Framework.Print("Attack results", `{Results.Text()}\n\n{ByAttack.Text()}\n\n{table.concat(Examples, "\n")}`)
		return true
	end)
end

local function Client()
	local function Frames(Count: number)
		for _ = 1, Count do
			RunService.Heartbeat:Wait()
		end
	end

	local function Fire(Remote: Instance, Payload: Payload)
		local function Send()
			local Target = Remote :: any
			local Ok, Error = pcall(function()
				if Payload.n > 7997 then
					if Remote:IsA("RemoteFunction") then
						return Target:InvokeServer(
							Payload[1],
							Payload[2],
							Payload[3],
							Payload[4],
							table.unpack(Payload, 5, Payload.n)
						)
					else
						Target:FireServer(
							Payload[1],
							Payload[2],
							Payload[3],
							Payload[4],
							table.unpack(Payload, 5, Payload.n)
						)
					end
				else
					if Remote:IsA("RemoteFunction") then
						return Target:InvokeServer(table.unpack(Payload, 1, Payload.n))
					else
						Target:FireServer(table.unpack(Payload, 1, Payload.n))
					end
				end
			end)
			if not Ok then
				warn(`Bench client rejection: {Remote:GetFullName()} n={Payload.n}: {Error}`)
			end
		end
		if Remote:IsA("RemoteFunction") then
			task.spawn(Send)
		else
			Send()
		end
	end

	local function SendEach(Library: Framework.Library)
		for _, Name in Library.Tests do
			Library.Fire[Name](Framework.Pool(Name)[1])
			Frames(3)
		end
		Frames(3)
	end

	for _, Tool in Framework.Libraries() do
		print(`Bench: attacking {Tool}`)
		local Library = Framework.Library(Tool)
		Framework.Call("Phase", "Prime", Tool)
		SendEach(Library)
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
		SendEach(Library)
		task.wait(0.3)
		Framework.Call("Phase", "Done", Tool)
	end
	Framework.Call("Report")
end

return { Server = Server, Client = Client }
