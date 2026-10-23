--!strict

export type Config = {
	Run: "Throughput" | "Attack",
	Libraries: { string }?,
	Tests: { string }?,
	Seconds: number,
	Rounds: number,
	FiresPerFrame: number,
}

local Config: Config = {
	Run = "Throughput",
	Libraries = nil,
	Tests = nil,
	Seconds = 1,
	Rounds = 2,
	FiresPerFrame = 250,
}

return Config
