--!strict
--!native
--!optimize 2

local ReplicatedStorage = game:GetService("ReplicatedStorage")

local Framework = require("../Framework")

local COPIES = 64
local WARM = 200
local BATCH_SECONDS = 0.01
local BATCHES = 3
local ROUNDS = 5

local function Fastest(Step: () -> ()): number
	for _ = 1, WARM do
		Step()
	end
	local Count = 1
	while true do
		local Start = os.clock()
		for _ = 1, Count do
			Step()
		end
		if os.clock() - Start >= BATCH_SECONDS then
			break
		end
		Count *= 2
	end
	local Best = math.huge
	for _ = 1, BATCHES do
		task.wait()
		local Start = os.clock()
		for _ = 1, Count do
			Step()
		end
		Best = math.min(Best, (os.clock() - Start) / Count)
	end
	return Best
end

local function Measure(Codec: any, Test: string, Pool: { any }): (number, number)
	local Write = Codec.Write[Test]
	local Stream = Codec.NewStream(8388608)
	local Size = #Pool
	local Next = 0
	local Written = 0
	local Encode = Fastest(function()
		Next = Next % Size + 1
		Write(Stream, Pool[Next])
		Written += 1
		if Written == COPIES then
			Codec.Finish(Stream)
			Written = 0
		end
	end)
	Codec.Finish(Stream)
	for Index = 1, COPIES do
		Write(Stream, Pool[Index])
	end
	local Frame = Codec.Finish(Stream)
	local Sender = {}
	assert(Codec.ReadOnServer(Frame, Sender), `{Test} refused its own frame`)
	local Decode = Fastest(function()
		Codec.ReadOnServer(Frame, Sender)
	end) / COPIES
	return Encode, Decode
end

local function Server()
	local Variants: { { Name: string, Codec: any } } = {}
	for _, Module in ReplicatedStorage.Bench.Vendor:WaitForChild("cpu"):GetChildren() do
		if Module:IsA("ModuleScript") then
			local Codec = require(Module) :: any
			for _, Test in Framework.Tests() do
				Codec.ServerHandlers[Test] = function() end
			end
			table.insert(Variants, { Name = Module.Name, Codec = Codec })
		end
	end
	table.sort(Variants, function(A: { Name: string, Codec: any }, B: { Name: string, Codec: any }): boolean
		return A.Name < B.Name
	end)
	local Rows = Framework.Table({ "Test", "Codec", "Encode µs", "Decode µs" })
	for _, Test in Framework.Tests() do
		local Encodes: { [string]: { number } } = {}
		local Decodes: { [string]: { number } } = {}
		for _, Variant in Variants do
			Encodes[Variant.Name] = {}
			Decodes[Variant.Name] = {}
		end
		for Round = 1, ROUNDS do
			for _, Variant in Framework.Shuffled(Variants, Round) do
				local Encode, Decode = Measure(Variant.Codec, Test, Framework.Pool(Test))
				table.insert(Encodes[Variant.Name], Encode * 1e6)
				table.insert(Decodes[Variant.Name], Decode * 1e6)
			end
		end
		for _, Variant in Variants do
			Rows.Row(
				Test,
				Variant.Name,
				string.format("%.2f", Framework.Median(Encodes[Variant.Name])),
				string.format("%.2f", Framework.Median(Decodes[Variant.Name]))
			)
		end
	end
	Framework.Print("Cpu results, per message", Rows.Text())
end

local function Client() end

return { Server = Server, Client = Client }
