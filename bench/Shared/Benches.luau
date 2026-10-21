--!strict

math.randomseed(0)

local Entities = table.create(100)
for Index = 1, 100 do
	Entities[Index] = {
		id = math.random(1, 255),
		x = math.random(1, 255),
		y = math.random(1, 255),
		z = math.random(1, 255),
		orientation = math.random(1, 255),
		animation = math.random(1, 255),
	}
end

local LETTERS = "abcdefghijklmnopqrstuvwxyz"
local Strings = table.create(300)
for Index = 1, 300 do
	local Text = ""
	for _ = 1, 8 do
		local Letter = math.random(1, 26)
		Text ..= string.sub(LETTERS, Letter, Letter)
	end
	Strings[Index] = Text
end

local Numbers = table.create(1000)
for Index = 1, 1000 do
	Numbers[Index] = math.random(0, 255)
end

local Dict = {}
for Index = 1, 200 do
	Dict[`Key{Index}`] = math.random(0, 255)
end

local Vectors = table.create(100)
for Index = 1, 100 do
	Vectors[Index] = Vector3.new(math.random() * 100, math.random() * 100, math.random() * 100)
end

return {
	Booleans = table.create(1000, true),
	Entities = Entities,
	Strings = Strings,
	Numbers = Numbers,
	Dict = Dict,
	Vectors = Vectors,
}
