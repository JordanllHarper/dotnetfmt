// Formatted using dotnetfmt format Unformatted.cs
public static int Add(int a, int b)
{
	int sum = a + b;
	// NOTE: some features aren't implemented yet, such as if expression parsing
	if (sum>0)
	{
		Console.WriteLine("positive: " + sum);
	}
	// NOTE: or new expressions
	List<int> seen=new List<int>();
	var ok = sum != 0 && sum < 100;
	sum += 1;
	return sum;
}
