--!strict

local ReplicatedStorage = game:GetService("ReplicatedStorage")

local Config = require(ReplicatedStorage.Bench.Config)

local RUNS = {
	Throughput = require(ReplicatedStorage.Bench.Runs.Throughput),
	Attack = require(ReplicatedStorage.Bench.Runs.Attack),
}

RUNS[Config.Run].Server()
