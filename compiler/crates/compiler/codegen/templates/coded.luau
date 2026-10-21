local function AppendBits(Stream: Stream, Cursor: number, Value: number, Bits: number): number
	local Byte = bit32.rshift(Cursor, 3)
	local Shift = bit32.band(Cursor, 7)
	local Kept = bit32.band(buffer.readu8(Stream, Byte), bit32.rshift(255, 8 - Shift))
	buffer.writeu32(Stream, Byte, bit32.bor(Kept, bit32.lshift(Value, Shift)))
	return Cursor + Bits
end

local function WriteCoded(Stream: Stream, Cursor: number, Value: number, K: number): number
	local Shifted = Value + bit32.lshift(1, K)
	local N = 31 - bit32.countlz(Shifted)
	local Zeros = N - K
	local Low = Shifted - bit32.lshift(1, N)
	if Zeros + Zeros + K < 25 then
		return AppendBits(Stream, Cursor, bit32.lshift(1, Zeros) + Low * bit32.lshift(2, Zeros), Zeros + 1 + N)
	end
	while Zeros > 24 do
		Cursor = AppendBits(Stream, Cursor, 0, 24)
		Zeros -= 24
	end
	Cursor = AppendBits(Stream, Cursor, bit32.lshift(1, Zeros), Zeros + 1)
	while N > 0 do
		local Take = math.min(N, 16)
		Cursor = AppendBits(Stream, Cursor, bit32.band(Low, bit32.lshift(1, Take) - 1), Take)
		Low = bit32.rshift(Low, Take)
		N -= Take
	end
	return Cursor
end

local function Peek(Source: buffer, Cursor: number): number
	return bit32.rshift(buffer.readu32(Source, bit32.rshift(Cursor, 3)), bit32.band(Cursor, 7))
end

local function ReadCoded(Source: buffer, Cursor: number, K: number): (number, number)
	local Window = Peek(Source, Cursor)
	local Zeros = bit32.countrz(Window)
	local N = Zeros + K
	if Zeros + 1 + N <= 25 then
		return bit32.extract(Window, Zeros + 1, N) + bit32.lshift(1, N) - bit32.lshift(1, K), Cursor + Zeros + 1 + N
	end
	Zeros = 0
	while true do
		local Run = bit32.countrz(Peek(Source, Cursor + Zeros))
		if Run < 25 then
			Zeros += Run
			break
		end
		Zeros += 25
		if Zeros > 31 then
			return -1, Cursor
		end
	end
	N = Zeros + K
	if N > 31 then
		return -1, Cursor
	end
	Cursor += Zeros + 1
	local Value = 0
	local Got = 0
	while Got < N do
		local Take = math.min(N - Got, 16)
		Value += bit32.extract(Peek(Source, Cursor), 0, Take) * 2 ^ Got
		Cursor += Take
		Got += Take
	end
	return Value + 2 ^ N - 2 ^ K, Cursor
end
