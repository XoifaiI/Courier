--!strict

--[=[
	Middlewares: the middleware Courier ships, each built from the runtime's Middleware maker so it
	does not require the runtime back

	Return type: a function from the runtime to the table of middleware
	Example usage:
		Net.Buy.AddMiddleware(Courier.Middlewares.Ratelimiter)
--]=]

local Types = require("./Types")

type Middlewares = Types.Middlewares
type Maker = { Middleware: <T, A>(Callback: Types.MiddlewareCallback<T, A>, Template: T?) -> Types.Middleware<T, A> }

return function(Courier: Maker): Middlewares
	return table.freeze({
		Ratelimiter = require("@self/Ratelimiter")(Courier),
	})
end
