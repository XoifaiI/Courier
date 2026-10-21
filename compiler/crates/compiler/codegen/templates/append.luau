local function AppendTo(Stream: Stream, From: Stream, FromEnd: number)
	local Bits = buffer.readu32(From, 0) - HEADER_BITS
	local Mark = buffer.readu32(From, 16)
	local Lead = if Mark > 0 then Mark - 1 else Bits
	local Rest = if Mark > 0 then Bits - bit32.band(Lead + 7, 0xFFFFFFF8) else 0
	local FromTop = buffer.readu32(From, 8)
	local Bytes = FromEnd - FromTop
	local Cursor = buffer.readu32(Stream, 0)
	local Top = buffer.readu32(Stream, 8)
	local Body = if Mark > 0 then bit32.band(Cursor + Lead + 7, 0xFFFFFFF8) else Cursor + Bits
	if Body + Rest > (if Bytes > 0 then (Top - Bytes) * 8 - GAP else buffer.readu32(Stream, 4)) then
		Full(Stream, Bits, Bytes)
		Cursor = buffer.readu32(Stream, 0)
		Top = buffer.readu32(Stream, 8)
		Body = if Mark > 0 then bit32.band(Cursor + Lead + 7, 0xFFFFFFF8) else Cursor + Bits
	end
	local Byte = Cursor // 8
	local Shift = bit32.band(Cursor, 7)
	local P = bit32.lshift(1, Shift)
	local Carry = bit32.band(buffer.readu8(Stream, Byte), P - 1)
	local Words = bit32.rshift(Lead + 31, 5)
	for Index = 0, Words - 1 do
		local Scaled = buffer.readu32(From, HEADER + Index * 4) * P
		local High = math.floor(Scaled * 2.3283064365386963e-10)
		buffer.writeu32(Stream, Byte + Index * 4, Scaled - High * 4294967296 + Carry)
		Carry = High
	end
	if Shift + Lead > Words * 32 then
		buffer.writeu8(Stream, Byte + Words * 4, Carry)
	end
	if Mark > 0 then
		local Ended = Cursor + Lead
		local Kept = bit32.band(Ended, 7)
		if Kept > 0 then
			local Last = Ended // 8
			buffer.writeu8(Stream, Last, bit32.band(buffer.readu8(Stream, Last), bit32.lshift(1, Kept) - 1))
		end
		if Rest > 0 then
			local From8 = (HEADER_BITS + Bits - Rest) // 8
			buffer.copy(Stream, Body // 8, From, From8, bit32.rshift(Rest + 7, 3))
		end
	end
	buffer.writeu32(Stream, 0, Body + Rest)
	if Bytes > 0 then
		Top -= Bytes
		if Bytes > 64 then
			buffer.copy(Stream, Top, From, FromTop, Bytes)
		else
			local Index = 0
			while Index + 4 <= Bytes do
				buffer.writeu32(Stream, Top + Index, buffer.readu32(From, FromTop + Index))
				Index += 4
			end
			while Index < Bytes do
				buffer.writeu8(Stream, Top + Index, buffer.readu8(From, FromTop + Index))
				Index += 1
			end
		end
		buffer.writeu32(Stream, 8, Top)
		buffer.writeu32(Stream, 4, Top * 8 - GAP)
	end
	local Count = buffer.readu32(From, 12)
	if Count > 0 then
		local Base = buffer.readu32(Stream, 12)
		local List = STREAM_INSTANCES[Stream]
		if List == nil then
			List = table.create(64)
			STREAM_INSTANCES[Stream] = List
		end
		table.move(STREAM_INSTANCES[From], 1, Count, Base + 1, List)
		buffer.writeu32(Stream, 12, Base + Count)
	end
	if Mark > 0 and buffer.readu32(Stream, 16) == 0 then
		buffer.writeu32(Stream, 16, Cursor - HEADER_BITS + Lead + 1)
	end
end
