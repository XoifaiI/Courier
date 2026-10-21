--!strict

--[=[
	Ratelimiter: lets each player make 20 calls to an event at once, refilling 5 a second, and
	rejects the rest. Calls from the server always pass

	Return type: a function from the runtime to the middleware
	Example usage:
		Net.Buy.AddMiddleware(Courier.Middlewares.Ratelimiter)
--]=]

local Types = require("../Types")

type Bucket = Types.Bucket
type Middleware<T> = Types.Middleware<T>
type RateLimiter = Types.RateLimiter
type Maker = { Middleware: <T>(Callback: Types.MiddlewareCallback<T>, Template: T?) -> Middleware<T> }

local function Refill(Bucket: Bucket, Now: number, Capacity: number, Rate: number)
	local Elapsed = Now - Bucket.Last

	if Elapsed > 0 then
		Bucket.Tokens = math.min(Capacity, Bucket.Tokens + Elapsed * Rate)
		Bucket.Last = Now
	end
end

local function IsAllowed(Ratelimiter: RateLimiter, Player: Player): boolean
	local Now = time()
	local Bucket = Ratelimiter.Buckets[Player]

	if not Bucket then
		Bucket = { Tokens = Ratelimiter.Capacity, Last = Now }
		Ratelimiter.Buckets[Player] = Bucket
	else
		Refill(Bucket, Now, Ratelimiter.Capacity, Ratelimiter.RefillPerSecond)
	end

	if Bucket.Tokens >= 1 then
		Bucket.Tokens -= 1
		return true
	end

	return false
end

return function(Courier: Maker): Middleware<RateLimiter>
	return Courier.Middleware(
		function(Call, Data)
			local Metadata = Call.Metadata

			local Player = Metadata.Player
			if not Player or Metadata.Origin == "Server" then
				return
			end

			if not IsAllowed(Data, Player) then
				Call:Reject("rate limited")
			end
		end,
		function(): RateLimiter
			return {
				Capacity = 20,
				RefillPerSecond = 5,
				Buckets = setmetatable({}, { __mode = "k" }) :: any,
			}
		end :: any
	)
end
