--!strict

local ReplicatedStorage = game:GetService("ReplicatedStorage")

local Framework = require(ReplicatedStorage.Bench.Framework) :: any

local Runs = {
	Throughput = require(ReplicatedStorage.Bench.Runs.Throughput),
	Game = require(ReplicatedStorage.Bench.Runs.Game),
	Attack = require(ReplicatedStorage.Bench.Runs.Attack),
}

Runs[Framework.Config.Run].Server(Framework)
