--!strict

local ReplicatedStorage = game:GetService("ReplicatedStorage")

local Config = require(ReplicatedStorage.Bench.Config)

local RUNS = {
	Throughput = require(ReplicatedStorage.Bench.Runs.Throughput),
	Game = require(ReplicatedStorage.Bench.Runs.Game),
	Attack = require(ReplicatedStorage.Bench.Runs.Attack),
	Cpu = require(ReplicatedStorage.Bench.Runs.Cpu),
}

RUNS[Config.Run].Server()
