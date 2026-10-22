--!strict

local POOL = 1024
local LETTERS = "abcdefghijklmnopqrstuvwxyz"

local Rng = Random.new(0)

local function Word(Shortest: number, Longest: number): string
	local Letters = table.create(Longest)
	for Index = 1, Rng:NextInteger(Shortest, Longest) do
		local Letter = Rng:NextInteger(1, 26)
		Letters[Index] = string.sub(LETTERS, Letter, Letter)
	end
	return table.concat(Letters)
end

local function Pool(Make: () -> any): { any }
	local Out = table.create(POOL)
	for Index = 1, POOL do
		Out[Index] = Make()
	end
	return Out
end

return {
	Booleans = Pool(function()
		local Out = table.create(1000)
		for Index = 1, 1000 do
			Out[Index] = Rng:NextInteger(0, 1) == 1
		end
		return Out
	end),
	Entities = Pool(function()
		local Out = table.create(100)
		for Index = 1, 100 do
			Out[Index] = {
				id = Rng:NextInteger(0, 255),
				x = Rng:NextInteger(0, 255),
				y = Rng:NextInteger(0, 255),
				z = Rng:NextInteger(0, 255),
				orientation = Rng:NextInteger(0, 255),
				animation = Rng:NextInteger(0, 255),
			}
		end
		return Out
	end),
	Strings = Pool(function()
		local Out = table.create(300)
		for Index = 1, 300 do
			Out[Index] = Word(4, 16)
		end
		return Out
	end),
	Numbers = Pool(function()
		local Out = table.create(1000)
		for Index = 1, 1000 do
			Out[Index] = Rng:NextInteger(0, 255)
		end
		return Out
	end),
	Dict = Pool(function()
		local Out = {}
		local Count = 0
		while Count < 200 do
			local Key = Word(4, 12)
			if Out[Key] == nil then
				Out[Key] = Rng:NextInteger(0, 255)
				Count += 1
			end
		end
		return Out
	end),
	Vectors = Pool(function()
		local Out = table.create(100)
		for Index = 1, 100 do
			Out[Index] = Vector3.new(Rng:NextNumber() * 100, Rng:NextNumber() * 100, Rng:NextNumber() * 100)
		end
		return Out
	end),
} :: { [string]: { any } }
