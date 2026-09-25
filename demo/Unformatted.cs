public   static   int Add(int a,int b)
{
int sum=a+b;
if (sum>0)
{
Console.WriteLine("positive: "+sum);
}
List<int> seen=new List<int>();
var ok=sum!=0&&sum<100;
sum+=1;
return sum;
}
