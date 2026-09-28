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
				List<int> seen=new List<int>();
				var ok = sum != 0 && sum < 100;
				sum += 1;
				return sum;
		}
}
