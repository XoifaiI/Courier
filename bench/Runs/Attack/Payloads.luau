--!strict

export type Payload = { n: number, [number]: any }

local CATEGORIES = {
	"Junk",
	"Buffers",
	"Truncated",
	"Flipped",
	"Overwritten",
	"Tables",
	"Sparse",
	"Arguments",
	"NilArguments",
	"ManyArguments",
	"JSON",
}

local Rng = Random.new(1)

local function Pack(...: any): Payload
	return table.pack(...) :: any
end

local function RandomBuffer(Length: number): buffer
	local Data = buffer.create(Length)
	for Index = 0, Length - 1 do
		buffer.writeu8(Data, Index, Rng:NextInteger(0, 255))
	end
	return Data
end

local function Filled(Length: number, Byte: number): buffer
	local Data = buffer.create(Length)
	buffer.fill(Data, 0, Byte)
	return Data
end

local function Resized(Data: buffer, Length: number): buffer
	local Out = buffer.create(Length)
	buffer.copy(Out, 0, Data, 0, math.min(Length, buffer.len(Data)))
	return Out
end

local function Nested(Depth: number): { any }
	local Root = {}
	local At = Root
	for _ = 1, Depth do
		local Next = {}
		At[1] = Next
		At = Next
	end
	return Root
end

local function Size(Value: any, Depth: number?): number
	local Level = Depth or 0
	if typeof(Value) == "buffer" then
		return buffer.len(Value)
	elseif type(Value) == "string" then
		return #Value
	elseif type(Value) == "table" and Level < 64 then
		local Total = 0
		for Key, Field in Value do
			Total += Size(Key, Level + 1) + Size(Field, Level + 1)
		end
		return Total
	end
	return 8
end

local function DeepCopy(Value: any): any
	if type(Value) ~= "table" then
		return Value
	end
	local Copy = {}
	for Key, Field in Value do
		Copy[Key] = DeepCopy(Field)
	end
	return Copy
end

local LEAVES: { any } = { 0 / 0, math.huge, -1, 300, 1e9, 2 ^ 53, "x", string.rep("a", 500), true, {}, Nested(50) }

local function Mutated(Value: any): any
	local Copy = DeepCopy(Value)
	local Tables = { Copy }
	local Index = 1
	while Index <= #Tables and #Tables < 4096 do
		for _, Field in Tables[Index] do
			if type(Field) == "table" then
				table.insert(Tables, Field)
			end
		end
		Index += 1
	end
	local Chosen = Tables[Rng:NextInteger(1, #Tables)]
	local Keys = {}
	for Key in Chosen do
		table.insert(Keys, Key)
	end
	if #Keys > 0 then
		Chosen[Keys[Rng:NextInteger(1, #Keys)]] = LEAVES[Rng:NextInteger(1, #LEAVES)]
	end
	return Copy
end

local GAPS = { 1000, 65536, 1000000, 2 ^ 31 - 1 }

local function Sparse(Original: { any }): { { any } }
	local First = Original[1]
	local Out = {}
	for _, Gap in GAPS do
		table.insert(Out, { [1] = First, [Gap] = {} })
		local Padded = table.clone(Original)
		Padded[#Original + Gap] = {}
		table.insert(Out, Padded)
	end
	table.insert(Out, { [0] = First, [-1] = {} })
	table.insert(Out, { [1] = First, [3] = First })
	return Out
end

local function EachBuffer(Samples: { Payload }, Make: (buffer) -> { buffer }): { Payload }
	local Out = {}
	for _, Sample in Samples do
		for Index = 1, Sample.n do
			local Arg = Sample[Index]
			if typeof(Arg) == "buffer" then
				for _, Replacement in Make(Arg) do
					local Copy = table.clone(Sample) :: Payload
					Copy[Index] = Replacement
					table.insert(Out, Copy)
				end
			end
		end
	end
	return Out
end

local function EachValue(Samples: { Payload }, Values: { any }): { Payload }
	local Out: { Payload } = {}
	for _, Value in Values do
		table.insert(Out, Pack(Value))
		for _, Sample in Samples do
			for Index = 1, Sample.n do
				local Copy = table.clone(Sample) :: Payload
				Copy[Index] = Value
				table.insert(Out, Copy)
			end
		end
	end
	return Out
end

local BUILD: { [string]: (Samples: { Payload }) -> { Payload } } = {
	Junk = function()
		local Out = { Pack() }
		for _, Value in
			{
				0,
				-1,
				0 / 0,
				math.huge,
				-math.huge,
				2 ^ 53,
				-2 ^ 53,
				1e308,
				"",
				"x",
				string.rep("\255", 1000),
				true,
				false,
				Vector3.new(0 / 0, math.huge, 0),
				CFrame.new(),
				workspace,
				Enum.KeyCode.A,
				Color3.new(),
				UDim2.new(),
			} :: { any }
		do
			table.insert(Out, Pack(Value))
		end
		return Out
	end,
	Buffers = function()
		local Out = {}
		for _, Length in { 0, 1, 2, 3, 4, 7, 8, 15, 16, 63, 64, 255, 256, 1023, 1024, 4096, 65535 } do
			table.insert(Out, Pack(RandomBuffer(Length)))
		end
		table.insert(Out, Pack(Filled(64, 255)))
		table.insert(Out, Pack(Filled(64, 0)))
		table.insert(Out, Pack(Filled(4096, 255)))
		return Out
	end,
	Truncated = function(Samples)
		return EachBuffer(Samples, function(Data)
			local Length = buffer.len(Data)
			local Out = {}
			local Seen = {}
			for _, Cut in { 0, 1, 2, 3, Length // 4, Length // 2, Length - 2, Length - 1 } do
				if Cut >= 0 and Cut < Length and not Seen[Cut] then
					Seen[Cut] = true
					table.insert(Out, Resized(Data, Cut))
				end
			end
			return Out
		end)
	end,
	Flipped = function(Samples)
		return EachBuffer(Samples, function(Data)
			local Length = buffer.len(Data)
			local Out = {}
			if Length == 0 then
				return Out
			end
			for Variant = 1, 24 do
				local Copy = Resized(Data, Length)
				for _ = 1, (Variant - 1) % 4 + 1 do
					local Byte = Rng:NextInteger(0, Length - 1)
					buffer.writeu8(
						Copy,
						Byte,
						bit32.bxor(buffer.readu8(Copy, Byte), bit32.lshift(1, Rng:NextInteger(0, 7)))
					)
				end
				table.insert(Out, Copy)
			end
			return Out
		end)
	end,
	Overwritten = function(Samples)
		return EachBuffer(Samples, function(Data)
			local Length = buffer.len(Data)
			local Out = {}
			if Length == 0 then
				return Out
			end
			for _ = 1, 12 do
				local Copy = Resized(Data, Length)
				for _ = 1, 2 do
					buffer.writeu8(Copy, Rng:NextInteger(0, Length - 1), 255)
				end
				table.insert(Out, Copy)
			end
			local Head = Resized(Data, Length)
			buffer.fill(Head, 0, 255, math.min(4, Length))
			table.insert(Out, Head)
			local Tail = Resized(Data, Length)
			buffer.fill(Tail, math.max(Length - 4, 0), 255)
			table.insert(Out, Tail)
			table.insert(Out, RandomBuffer(Length))
			table.insert(Out, Filled(Length, 255))
			local Longer = Resized(Data, Length + 16)
			buffer.copy(Longer, Length, RandomBuffer(16))
			table.insert(Out, Longer)
			return Out
		end)
	end,
	Tables = function(Samples)
		local Out = {
			Pack(Nested(100)),
			Pack(Nested(290)),
			Pack(table.create(50000, 0)),
			Pack({ [1] = 1, [1000000] = 1 }),
			Pack({ 1, 2, a = 3 }),
			Pack({ 0 / 0, math.huge, -math.huge }),
			Pack(table.create(100, string.rep("z", 10000))),
			Pack({ { { { 0 / 0 } } } }),
		}
		local Entities = table.create(1000)
		for Index = 1, 1000 do
			Entities[Index] = { id = -1, x = 300, y = 0 / 0, z = "x", orientation = {}, animation = true }
		end
		table.insert(Out, Pack(Entities))
		for _, Sample in Samples do
			for Index = 1, Sample.n do
				if type(Sample[Index]) == "table" then
					for _ = 1, 16 do
						local Copy = table.clone(Sample) :: Payload
						Copy[Index] = Mutated(Sample[Index])
						table.insert(Out, Copy)
					end
				end
			end
		end
		return Out
	end,
	Sparse = function(Samples)
		local Out = {}
		for _, Sample in Samples do
			local Found = false
			for Index = 1, Sample.n do
				local Arg = Sample[Index]
				if type(Arg) == "table" then
					Found = true
					for _, Replacement in Sparse(Arg) do
						local Copy = table.clone(Sample) :: Payload
						Copy[Index] = Replacement
						table.insert(Out, Copy)
					end
				end
			end
			if not Found then
				for _, Extra in Sparse({ workspace }) do
					local Copy = table.clone(Sample) :: Payload
					Copy.n += 1
					Copy[Copy.n] = Extra
					table.insert(Out, Copy)
				end
			end
		end
		return Out
	end,
	Arguments = function(Samples)
		local Out = {
			Pack(RandomBuffer(64), RandomBuffer(64)),
			Pack(RandomBuffer(64), "junk"),
			Pack(RandomBuffer(64), 0 / 0),
			Pack(RandomBuffer(64), { workspace, workspace }),
			Pack(RandomBuffer(64), table.create(1000, workspace)),
			Pack(table.unpack(table.create(20, RandomBuffer(16)))),
		}
		for _, Sample in Samples do
			local Extra = table.clone(Sample) :: Payload
			Extra.n += 1
			Extra[Extra.n] = RandomBuffer(32)
			table.insert(Out, Extra)
			for Index = 1, Sample.n do
				local Missing = table.clone(Sample) :: Payload
				Missing[Index] = nil
				table.insert(Out, Missing)
			end
		end
		return Out
	end,
	NilArguments = function(Samples)
		local Out: { Payload } = {
			Pack(nil),
			Pack(nil, nil),
			Pack(nil, nil, nil),
			Pack("number", nil, "string"),
			Pack("number", nil, "string", nil),
			Pack("number", nil, "string", nil, nil),
			Pack(nil, "string", nil),
		}
		for _, Sample in Samples do
			for _, Extra in { 1, 2, 8, 32 } do
				local Copy = table.clone(Sample) :: Payload
				Copy.n = Sample.n + Extra
				table.insert(Out, Copy)
			end
			for Position = 1, Sample.n + 1 do
				local Copy: Payload = { n = Sample.n + 1 }
				for Index = 1, Sample.n do
					local Destination = if Index < Position then Index else Index + 1
					Copy[Destination] = Sample[Index]
				end
				table.insert(Out, Copy)
			end
			for Index = 1, Sample.n do
				local Copy = table.clone(Sample) :: Payload
				Copy[Index] = nil
				table.insert(Out, Copy)
			end
			for Keep = 0, Sample.n do
				local Copy: Payload = { n = Sample.n }
				for Index = 1, Keep do
					Copy[Index] = Sample[Index]
				end
				table.insert(Out, Copy)
			end
		end
		return Out
	end,
	ManyArguments = function(Samples)
		local Out: { Payload } = {}
		for _, Count in { 7999, 8000, 8001 } do
			local Dense = table.create(Count, 0) :: any
			Dense.n = Count
			table.insert(Out, Dense :: Payload)
			local AllNil: Payload = { n = Count }
			table.insert(Out, AllNil)
			local Holes: Payload = { n = Count }
			Holes[1] = "first"
			Holes[Count - 1] = "last non-nil"
			table.insert(Out, Holes)
			for _, Sample in Samples do
				if Sample.n <= Count then
					local Extra = table.clone(Sample) :: Payload
					for Index = Sample.n + 1, Count do
						Extra[Index] = 0
					end
					Extra.n = Count
					table.insert(Out, Extra)
					local Trailing = table.clone(Sample) :: Payload
					Trailing.n = Count
					table.insert(Out, Trailing)
				end
			end
		end
		return Out
	end,
	JSON = function(Samples)
		return EachValue(Samples, {
			0 / 0,
			math.huge,
			-math.huge,
			{ value = 0 / 0 },
			{ value = math.huge },
			{ value = -math.huge },
			{ [1] = "array", label = "object" },
			{ [1] = "numeric", ["1"] = "string" },
			{ [1] = "first", [3] = "third" },
			{ [0] = "zero", [-1] = "negative", [1] = "one" },
			{ n = 3, [1] = "number", [3] = "string" },
			{ n = 5, [1] = "number", [3] = "string" },
			{ n = 32 },
			"\0",
			'"\\\n\r\t',
			"\255",
			"\192\175",
			"\237\160\128",
			{ value = "\255" },
			{ ["\255"] = "invalid UTF-8 key" },
			{ value = workspace },
			{ value = Vector3.new(1, 2, 3) },
			{ value = CFrame.new() },
			{ value = buffer.create(8) },
			"null",
			"[]",
			"{}",
			"[1,null,3,null]",
			'{"n":5,"1":"number","3":"string"}',
			'{"value":1,"value":2}',
			'{"value":1e309}',
			'{"value":NaN}',
			'{"value":Infinity}',
			'{"value":"\\uD800"}',
			'{"value":',
			"[1,]",
		})
	end,
}

return {
	CATEGORIES = CATEGORIES,
	Build = BUILD,
	Size = Size,
}
