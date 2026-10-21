local function Fits(Stream: Stream, Bits: number, Bytes: number): boolean
	if Bytes > 0 then
		return buffer.readu32(Stream, 0) + Bits <= (buffer.readu32(Stream, 8) - Bytes) * 8 - GAP
	end
	return buffer.readu32(Stream, 0) + Bits <= buffer.readu32(Stream, 4)
end

local function Full(Stream: Stream, Bits: number, Bytes: number)
	local Handler = OnFull
	if Handler == nil then
		error(`Courier: a stream of {buffer.len(Stream) - HEADER - 4} bytes is full and no full handler is set, so the message is dropped; call SetOnFull or make the stream larger`, 3)
	end
	Handler(Stream)
	if not Fits(Stream, Bits, Bytes) then
		error(`Courier: a message of {Bits} bits and {Bytes} bytes cannot fit a stream of {buffer.len(Stream) - HEADER - 4} bytes even when empty, so it is dropped`, 3)
	end
end
