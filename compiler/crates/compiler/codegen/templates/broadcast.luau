local BROADCAST = buffer.create(0)

local function Broadcast(Like: Stream): Stream
	local Scratch = BROADCAST
	if buffer.len(Scratch) < buffer.len(Like) then
		Scratch = NewStream(buffer.len(Like) - HEADER - 4)
		BROADCAST = Scratch
	end
	local End = buffer.len(Scratch)
	buffer.writeu32(Scratch, 0, HEADER_BITS)
	buffer.writeu32(Scratch, 4, (End - 4) * 8)
	buffer.writeu32(Scratch, 8, End)
	buffer.writeu32(Scratch, 12, 0)
	buffer.writeu32(Scratch, 16, 0)
	return Scratch
end
