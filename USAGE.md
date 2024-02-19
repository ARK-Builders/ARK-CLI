## Usage

Beginning with the following dir structure

```
├───ark-shelf
│   └───.ark
│       ├───cache
│       │   ├───metadata
│       │   └───previews
│       └───user
│           ├───properties
│           ├───scores
│           └───tags
```

First create some sample links

`ark-cli link create .\ark-shelf\ http://google.com google hi`

`ark-cli link create .\ark-shelf\ http://bing.com bing hello`

Then add some tags to the links

`ark-cli file append .\ark-shelf\ tags <resource_id> search,engine`

The same way we can append scores

`ark-cli file append .\ark-shelf\ scores <resource_id> 15`

We can also append json data

`ark-cli file append .\ark-shelf\ properties <resource_id> favorites:false,ai:true --format=json`

You can read these properties

`ark-cli file read .\ark-shelf\ properties <resource_id>`

Or the scores

`ark-cli file read .\ark-shelf\ scores <resource_id>`

You can list the entries for a storage like this

`ark-cli storage list .\ark-shelf\ properties`

For more info you can add the versions flag

`ark-cli storage list .\ark-shelf\ properties --versions=true`

Also works for file storages

`ark-cli storage list .\ark-shelf\ scores --versions=true`

List the files in the index using 

`ark-cli list .\ark-shelf\`

`--entry=id|path|both` -> to show the path,the id or both of a resource

`--timestamp=true` -> to show or not the last modified timestamp of a resource

`--tags=true` -> to show or not the tags for every resource

`--scores=true` -> to show or not the scores for every resource

`--sort=asc|desc` -> to sort resources by asc or dsc order of scores

`--filter=query` -> to filter resources by their tags





