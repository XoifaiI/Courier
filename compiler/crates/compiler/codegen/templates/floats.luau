local FLOAT_BITS = buffer.create(8)

local function F32Bits(Value: number): number
	buffer.writef32(FLOAT_BITS, 0, Value)
	return buffer.readu32(FLOAT_BITS, 0)
end

local function BitsF32(Bits: number): number
	buffer.writeu32(FLOAT_BITS, 0, Bits)
	return buffer.readf32(FLOAT_BITS, 0)
end

local function F64Bits(Value: number): (number, number)
	buffer.writef64(FLOAT_BITS, 0, Value)
	return buffer.readu32(FLOAT_BITS, 0), buffer.readu32(FLOAT_BITS, 4)
end

local function BitsF64(Low: number, High: number): number
	buffer.writeu32(FLOAT_BITS, 0, Low)
	buffer.writeu32(FLOAT_BITS, 4, High)
	return buffer.readf64(FLOAT_BITS, 0)
end
