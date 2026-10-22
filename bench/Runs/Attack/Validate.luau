--!strict

local function Byte(Value: any): boolean
	return type(Value) == "number" and Value == Value // 1 and Value >= 0 and Value <= 255
end

local function Text(Value: any): boolean
	return type(Value) == "string" and #Value <= 50
end

local function Finite(Value: number): boolean
	return Value == Value and Value ~= math.huge and Value ~= -math.huge
end

local function Vector(Value: any): boolean
	return typeof(Value) == "Vector3" and Finite(Value.X) and Finite(Value.Y) and Finite(Value.Z)
end

local function Entity(Value: any): boolean
	if type(Value) ~= "table" then
		return false
	end
	local Count = 0
	for Key, Field in Value do
		Count += 1
		if
			(Key ~= "id" and Key ~= "x" and Key ~= "y" and Key ~= "z" and Key ~= "orientation" and Key ~= "animation")
			or not Byte(Field)
		then
			return false
		end
	end
	return Count == 6
end

local function List(Value: any, Most: number, Item: (any) -> boolean): boolean
	if type(Value) ~= "table" then
		return false
	end
	local Length = #Value
	if Length > Most then
		return false
	end
	local Count = 0
	for _ in Value do
		Count += 1
	end
	if Count ~= Length then
		return false
	end
	for Index = 1, Length do
		if not Item(Value[Index]) then
			return false
		end
	end
	return true
end

local function Map(Value: any): boolean
	if type(Value) ~= "table" then
		return false
	end
	local Count = 0
	for Key, Field in Value do
		Count += 1
		if Count > 500 or not Text(Key) or not Byte(Field) then
			return false
		end
	end
	return true
end

local function Boolean(Value: any): boolean
	return type(Value) == "boolean"
end

local CHECKS: { [string]: (any) -> boolean } = {
	Booleans = function(Value)
		return List(Value, 1000, Boolean)
	end,
	Entities = function(Value)
		return List(Value, 1000, Entity)
	end,
	Strings = function(Value)
		return List(Value, 500, Text)
	end,
	Numbers = function(Value)
		return List(Value, 2000, Byte)
	end,
	Dict = Map,
	Vectors = function(Value)
		return List(Value, 500, Vector)
	end,
}

return function(Name: string, Value: any): boolean
	return CHECKS[Name](Value)
end
