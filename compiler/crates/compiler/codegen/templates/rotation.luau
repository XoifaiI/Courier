local HALF_SQRT2 = 0.7071067811865476
local SQRT2 = 1.4142135623730951

local function PackCFrame(Value: CFrame, Steps: number): (number, number, number, number, number, number, number)
	local PX, PY, PZ, R00, R01, R02, R10, R11, R12, R20, R21, R22 = Value:GetComponents()
	local X, Y, Z, W
	local Trace = R00 + R11 + R22
	if Trace > 0 then
		local S = math.sqrt(Trace + 1) * 2
		W, X, Y, Z = 0.25 * S, (R21 - R12) / S, (R02 - R20) / S, (R10 - R01) / S
	elseif R00 > R11 and R00 > R22 then
		local S = math.sqrt(1 + R00 - R11 - R22) * 2
		W, X, Y, Z = (R21 - R12) / S, 0.25 * S, (R01 + R10) / S, (R02 + R20) / S
	elseif R11 > R22 then
		local S = math.sqrt(1 + R11 - R00 - R22) * 2
		W, X, Y, Z = (R02 - R20) / S, (R01 + R10) / S, 0.25 * S, (R12 + R21) / S
	else
		local S = math.sqrt(1 + R22 - R00 - R11) * 2
		W, X, Y, Z = (R10 - R01) / S, (R02 + R20) / S, (R12 + R21) / S, 0.25 * S
	end
	local Index, A, B, C, Largest = 0, Y, Z, W, X
	if math.abs(Y) > math.abs(Largest) then
		Index, A, B, C, Largest = 1, X, Z, W, Y
	end
	if math.abs(Z) > math.abs(Largest) then
		Index, A, B, C, Largest = 2, X, Y, W, Z
	end
	if math.abs(W) > math.abs(Largest) then
		Index, A, B, C, Largest = 3, X, Y, Z, W
	end
	local Scale = if Largest < 0 then -Steps / SQRT2 else Steps / SQRT2
	local Offset = Steps * 0.5
	return PX,
		PY,
		PZ,
		Index,
		math.round(math.min(math.max(0, A * Scale + Offset), Steps)),
		math.round(math.min(math.max(0, B * Scale + Offset), Steps)),
		math.round(math.min(math.max(0, C * Scale + Offset), Steps))
end

local function UnpackRotation(Index: number, A: number, B: number, C: number, Steps: number): (number, number, number, number)
	local Scale = SQRT2 / Steps
	local P, Q, R = A * Scale - HALF_SQRT2, B * Scale - HALF_SQRT2, C * Scale - HALF_SQRT2
	local D = math.sqrt(math.max(0, 1 - P * P - Q * Q - R * R))
	if Index == 0 then
		return D, P, Q, R
	elseif Index == 1 then
		return P, D, Q, R
	elseif Index == 2 then
		return P, Q, D, R
	end
	return P, Q, R, D
end
