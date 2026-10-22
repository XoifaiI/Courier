local MAP_TEMPLATES: { { [any]: any } } = {}
for Power = 1, @MAP_POWER@ do
	local Template = {}
	for Index = 1, bit32.lshift(1, Power) do
		Template[-Index] = true
	end
	table.clear(Template)
	MAP_TEMPLATES[Power] = Template
end
