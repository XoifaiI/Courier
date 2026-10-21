--!strict

export type Config = {
	Run: "Throughput" | "Game" | "Attack",
	Tools: { string }?,
	Seconds: number,
	Rounds: number,
	FiresPerFrame: number,
	CourierCapacity: number,
}

local Config: Config = {
	Run = "Throughput",
	Tools = nil,
	Seconds = 2,
	Rounds = 3,
	FiresPerFrame = 250,
	CourierCapacity = 4194304,
}

return Config
