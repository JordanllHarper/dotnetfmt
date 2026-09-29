public static class Program
{
		public static int Add(int a, int b)
		{
				int sum = a + b;
				if (sum > 0)
				{
						Console.WriteLine("positive: " + sum);
				}
				else if (sum < 0)
				{
						Console.WriteLine("negative: " + sum);
				}
				else
				{
						Console.WriteLine("zero");
				}
				while (sum > 100)
				{
						sum -= 10;
				}
				do
				{
						sum -= 1;
				} while (sum > 0);
				List<int> seen = new List<int>();
				foreach (int n in seen)
				{
						Console.WriteLine(n);
				}
				var ok = sum != 0 && sum < 100;
				for (int i = 0; i < seen.Count; i++)
				{
						Console.WriteLine(i);
				}
				switch (sum)
				{
						case 0:
						Console.WriteLine("none");
						break;
						case > 0 and < 100:
						Console.WriteLine("small");
						break;
						case not 0 when sum >= 100:
						Console.WriteLine("big");
						break;
						default:
						Console.WriteLine("some");
						break;
				}
				var label = sum switch
				{
						0 => "none",
						> 0 and < 100 => "small",
						not 0 when sum >= 100 => "big",
						_ => "some",
				};
				var tag = sum switch { 0 => "none", > 0 and < 100 => "small", _ => "other" };
				sum += 1;
				return sum;
		}
}
