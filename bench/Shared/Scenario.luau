--!strict

local UNITS = 50
local WORDS = {
	"go",
	"left",
	"right",
	"push",
	"wait",
	"nice",
	"help",
	"base",
	"they",
	"are",
	"on",
	"the",
	"bridge",
	"heal",
	"me",
	"gg",
	"retreat",
	"flank",
	"now",
	"ok",
}

type Unit = { id: number, position: Vector3, yaw: number, health: number, velocity: Vector3, turn: number }
type Message = { any }

local function Server(Seed: number): () -> { Message }
	local Rng = Random.new(Seed)
	local Units: { Unit } = table.create(UNITS) :: any
	for Index = 1, UNITS do
		Units[Index] = {
			id = Index * 7,
			position = Vector3.new(Rng:NextNumber(-500, 500), Rng:NextNumber(0, 50), Rng:NextNumber(-500, 500)),
			yaw = Rng:NextInteger(0, 65535),
			health = 100,
			velocity = Vector3.new(Rng:NextNumber(-16, 16), 0, Rng:NextNumber(-16, 16)),
			turn = Rng:NextInteger(-300, 300),
		}
	end
	return function(): { Message }
		local State = table.create(UNITS)
		for Index, Unit in Units do
			if Rng:NextNumber() < 0.02 then
				Unit.velocity = Vector3.new(Rng:NextNumber(-16, 16), 0, Rng:NextNumber(-16, 16))
			end
			Unit.position += Unit.velocity / 60
			Unit.yaw = (Unit.yaw + Unit.turn) % 65536
			if Rng:NextNumber() < 0.01 then
				Unit.health = Rng:NextInteger(0, 100)
			end
			State[Index] = { id = Unit.id, position = Unit.position, yaw = Unit.yaw, health = Unit.health }
		end
		local Out: { Message } = { { "State", State } }
		for _ = 1, Rng:NextInteger(0, 4) do
			table.insert(Out, {
				"Hit",
				{
					attacker = Units[Rng:NextInteger(1, UNITS)].id,
					target = Units[Rng:NextInteger(1, UNITS)].id,
					damage = Rng:NextInteger(1, 100),
				},
			})
		end
		if Rng:NextNumber() < 0.05 then
			table.insert(Out, { "Pickup", { item = Rng:NextInteger(0, 4000), slot = Rng:NextInteger(0, 30) } })
		end
		if Rng:NextNumber() < 1 / 60 then
			local Words = {}
			for _ = 1, Rng:NextInteger(2, 12) do
				table.insert(Words, WORDS[Rng:NextInteger(1, #WORDS)])
			end
			table.insert(Out, { "Chat", table.concat(Words, " ") })
		end
		return Out
	end
end

local function Client(Seed: number): () -> { Message }
	local Rng = Random.new(Seed)
	local Look = Vector3.new(0, 0, -1)
	return function(): { Message }
		Look = (Look + Vector3.new(
			Rng:NextNumber(-0.05, 0.05),
			Rng:NextNumber(-0.02, 0.02),
			Rng:NextNumber(-0.05, 0.05)
		)).Unit
		local Moving = Rng:NextNumber() < 0.7
		return {
			{
				"Input",
				{
					move = if Moving
						then Vector3.new(Rng:NextNumber(-1, 1), 0, Rng:NextNumber(-1, 1)).Unit
						else Vector3.zero,
					look = Look,
					jump = Rng:NextNumber() < 0.05,
					fire = Rng:NextNumber() < 0.2,
					crouch = Rng:NextNumber() < 0.1,
				},
			},
		}
	end
end

local function Integer(Value: any, Most: number): boolean
	return type(Value) == "number" and Value == Value // 1 and Value >= 0 and Value <= Most
end

local function Finite(Value: any): boolean
	return typeof(Value) == "Vector3"
		and Value.X == Value.X
		and Value.Y == Value.Y
		and Value.Z == Value.Z
		and math.abs(Value.X) ~= math.huge
		and math.abs(Value.Y) ~= math.huge
		and math.abs(Value.Z) ~= math.huge
end

local function Exactly(Value: any, Fields: { [string]: (any) -> boolean }): boolean
	if type(Value) ~= "table" then
		return false
	end
	local Count = 0
	for Key, Field in Value do
		Count += 1
		local Check = Fields[Key]
		if Check == nil or not Check(Field) then
			return false
		end
	end
	local Expected = 0
	for _ in Fields do
		Expected += 1
	end
	return Count == Expected
end

local function U16(Value: any): boolean
	return Integer(Value, 65535)
end

local function U8(Value: any): boolean
	return Integer(Value, 255)
end

local function Boolean(Value: any): boolean
	return type(Value) == "boolean"
end

local UNIT = { id = U16, position = Finite, yaw = U16, health = U8 }

local CHECKS: { [string]: (any) -> boolean } = {
	State = function(Value)
		if type(Value) ~= "table" or #Value > 64 then
			return false
		end
		for _, Unit in Value do
			if not Exactly(Unit, UNIT) then
				return false
			end
		end
		return true
	end,
	Hit = function(Value)
		return Exactly(Value, { attacker = U16, target = U16, damage = U8 })
	end,
	Pickup = function(Value)
		return Exactly(Value, { item = U16, slot = U8 })
	end,
	Chat = function(Value)
		return type(Value) == "string" and #Value <= 200
	end,
	Input = function(Value)
		return Exactly(Value, { move = Finite, look = Finite, jump = Boolean, fire = Boolean, crouch = Boolean })
	end,
}

return {
	SERVER = { "State", "Hit", "Pickup", "Chat" },
	CLIENT = { "Input" },
	Server = Server,
	Client = Client,
	Check = function(Name: string, Value: any): boolean
		return CHECKS[Name](Value)
	end,
}
