local HEADER = 20
local HEADER_BITS = 160
local GAP = @GAP@
local READ_PAD = @READ_PAD@
local EMPTY = buffer.create(0)
local READ_SCRATCH = buffer.create(1024)
local STREAM_INSTANCES: { [Stream]: { any } } = setmetatable({}, { __mode = "k" }) :: any

local OnFull: ((Stream: Stream) -> ())? = nil

local function NewStream(Capacity: number): Stream
	local End = HEADER + math.max(Capacity, 16) + 4
	local Stream = buffer.create(End)
	buffer.writeu32(Stream, 0, HEADER_BITS)
	buffer.writeu32(Stream, 4, (End - 4) * 8)
	buffer.writeu32(Stream, 8, End)
	buffer.writeu32(Stream, 12, 0)
	buffer.writeu32(Stream, 16, 0)
	return Stream
end


local function SetOnFull(Handler: (Stream: Stream) -> ())
	OnFull = Handler
end

local function IsEmpty(Stream: Stream): boolean
	return buffer.readu32(Stream, 0) == HEADER_BITS and buffer.readu32(Stream, 8) == buffer.len(Stream)
end

@WRITERS@local function Finish(Stream: Stream): (buffer, { any }?)
	local Bits = buffer.readu32(Stream, 0) - HEADER_BITS
	local Top = buffer.readu32(Stream, 8)
	local End = buffer.len(Stream)
	local Tail = End - Top
	local BitBytes = bit32.rshift(Bits + 7, 3)
	local Frame = buffer.create(BitBytes + Tail)
	buffer.copy(Frame, 0, Stream, HEADER, BitBytes)
	local Last = bit32.rshift(Bits, 3)
	if Last < BitBytes then
		buffer.writeu8(Frame, Last, bit32.band(buffer.readu8(Frame, Last), bit32.rshift(255, 8 - bit32.band(Bits, 7))))
		buffer.fill(Frame, Last + 1, 0, BitBytes - Last - 1)
	end
	if Tail > 0 then
		buffer.copy(Frame, BitBytes, Stream, Top, Tail)
	end
	buffer.writeu32(Stream, 0, HEADER_BITS)
	buffer.writeu32(Stream, 4, (End - 4) * 8)
	buffer.writeu32(Stream, 8, End)
	buffer.writeu32(Stream, 16, 0)
	local Count = buffer.readu32(Stream, 12)
	if Count == 0 then
		return Frame, nil
	end
	buffer.writeu32(Stream, 12, 0)
	local Instances = STREAM_INSTANCES[Stream]
	STREAM_INSTANCES[Stream] = nil
	for Index = #Instances, Count + 1, -1 do
		Instances[Index] = nil
	end
	return Frame, Instances
end
